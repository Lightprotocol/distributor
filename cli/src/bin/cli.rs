extern crate jito_merkle_tree;
extern crate merkle_distributor;

use std::path::PathBuf;

use anchor_lang::{
    prelude::Pubkey, AccountDeserialize, AnchorDeserialize, InstructionData, Key, ToAccountMetas,
};
use anchor_spl::token;
use clap::{Parser, Subcommand};
use jito_merkle_tree::{
    airdrop_merkle_tree::AirdropMerkleTree,
    utils::{get_claim_status_pda, get_merkle_distributor_pda},
};
use light_client::{
    indexer::{AddressWithTree, Indexer},
    rpc::{LightClient, LightClientConfig, Rpc},
};
use light_sdk::instruction::{
    account_meta::CompressedAccountMeta, PackedAccounts, PackedStateTreeInfo,
    SystemAccountMetaConfig,
};
use merkle_distributor::state::{
    claim_status::{ClaimStatus, ClaimStatusInstructionData},
    merkle_distributor::MerkleDistributor,
};
use solana_program::instruction::Instruction;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};

const NEW_CLAIM_COMPUTE_UNITS: u32 = 400_000;
const CLAIM_LOCKED_COMPUTE_UNITS: u32 = 500_000;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Commands,

    /// Airdrop version
    #[clap(long, env, default_value_t = 0)]
    pub airdrop_version: u64,

    /// SPL Mint address
    #[clap(long, env)]
    pub mint: Pubkey,

    /// RPC url
    #[clap(long, env)]
    pub rpc_url: String,

    /// Photon indexer URL (defaults to RPC url if not specified)
    #[clap(long, env)]
    pub photon_url: Option<String>,

    /// Program id
    #[clap(long, env, default_value_t = merkle_distributor::id())]
    pub program_id: Pubkey,

    /// Payer keypair
    #[clap(long, env)]
    pub keypair_path: PathBuf,

    /// Priority fee
    #[clap(long, env)]
    pub priority: Option<u64>,
}

// Subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Claim unlocked tokens
    Claim(ClaimArgs),
    /// Create a new instance of a merkle distributor
    NewDistributor(NewDistributorArgs),
    /// Clawback tokens from merkle distributor
    #[clap(hide = true)]
    Clawback(ClawbackArgs),
    /// Create a Merkle tree, given a CSV of recipients
    CreateMerkleTree(CreateMerkleTreeArgs),
    SetAdmin(SetAdminArgs),
}

// NewClaim and Claim subcommand args
#[derive(Parser, Debug)]
pub struct ClaimArgs {
    /// Merkle distributor path
    #[clap(long, env)]
    pub merkle_tree_path: PathBuf,
}

// NewDistributor subcommand args
#[derive(Parser, Debug)]
pub struct NewDistributorArgs {
    /// Clawback receiver token account
    #[clap(long, env)]
    pub clawback_receiver_token_account: Pubkey,

    /// Lockup timestamp start
    #[clap(long, env)]
    pub start_vesting_ts: i64,

    /// Lockup timestamp end (unix timestamp)
    #[clap(long, env)]
    pub end_vesting_ts: i64,

    /// Merkle distributor path
    #[clap(long, env)]
    pub merkle_tree_path: PathBuf,

    /// When to make the clawback period start. Must be at least a day after the end_vesting_ts
    #[clap(long, env)]
    pub clawback_start_ts: i64,
}

#[derive(Parser, Debug)]
pub struct ClawbackArgs {
    #[clap(long, env)]
    pub clawback_keypair_path: PathBuf,
}

#[derive(Parser, Debug)]
pub struct CreateMerkleTreeArgs {
    /// CSV path
    #[clap(long, env)]
    pub csv_path: PathBuf,

    /// Merkle tree out path
    #[clap(long, env)]
    pub merkle_tree_path: PathBuf,
}

#[derive(Parser, Debug)]
pub struct SetAdminArgs {
    #[clap(long, env)]
    pub new_admin: Pubkey,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match &args.command {
        Commands::NewDistributor(new_distributor_args) => {
            process_new_distributor(&args, new_distributor_args);
        }
        Commands::Claim(claim_args) => {
            process_claim(&args, claim_args).await;
        }
        Commands::Clawback(clawback_args) => process_clawback(&args, clawback_args),
        Commands::CreateMerkleTree(merkle_tree_args) => {
            process_create_merkle_tree(merkle_tree_args);
        }
        Commands::SetAdmin(set_admin_args) => {
            process_set_admin(&args, set_admin_args);
        }
    }
}

async fn process_new_claim(args: &Args, claim_args: &ClaimArgs) {
    let keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");
    let claimant = keypair.pubkey();
    println!("Claiming tokens for user {}...", claimant);

    let merkle_tree = AirdropMerkleTree::new_from_file(&claim_args.merkle_tree_path)
        .expect("failed to load merkle tree from file");

    let (distributor, _bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);

    // Get user's node in claim
    let node = merkle_tree.get_node(&claimant);
    let (claim_status_address, _address_seed) = get_claim_status_pda(
        &args.program_id,
        &claimant,
        &distributor,
    );
    let address_tree = Pubkey::new_from_array(light_sdk::constants::ADDRESS_TREE_V2);

    let photon_url = args.photon_url.clone().unwrap_or_else(|| args.rpc_url.clone());
    let config = LightClientConfig {
        url: args.rpc_url.to_string(),
        photon_url: Some(photon_url),
        commitment_config: None,
        fetch_active_tree: true,
        api_key: None,
    };
    let mut client = LightClient::new(config).await.expect("failed to create client");

    let claimant_ata = get_associated_token_address(&claimant, &args.mint);

    let mut ixs = vec![ComputeBudgetInstruction::set_compute_unit_limit(NEW_CLAIM_COMPUTE_UNITS)];
    let proof = client
        .get_validity_proof(
            vec![],
            vec![AddressWithTree {
                address: claim_status_address,
                tree: address_tree,
            }],
            None,
        )
        .await
        .expect("failed to get validity proof")
        .value;

    let mut packed_accounts = PackedAccounts::default();
    packed_accounts.add_system_accounts_v2(SystemAccountMetaConfig::new(merkle_distributor::ID))
        .expect("add system accounts");

    // Pack address tree info for v2
    let address_tree_info = proof.pack_tree_infos(&mut packed_accounts).address_trees[0];
    let output_state_tree_index = client
        .get_random_state_tree_info()
        .expect("failed to get state tree info")
        .pack_output_tree_index(&mut packed_accounts)
        .expect("failed to pack output tree");

    match client.get_account(claimant_ata).await {
        Ok(_) => {}
        Err(e) => {
            if e.to_string().contains("AccountNotFound") {
                println!("PDA does not exist. creating.");
                let ix =
                    create_associated_token_account(&claimant, &claimant, &args.mint, &token::ID);
                ixs.push(ix);
            } else {
                eprintln!("Error fetching PDA: {e}");
                std::process::exit(1);
            }
        }
    }
    let (packed_account_metas, _, _) = packed_accounts.to_account_metas();

    let new_claim_ix = Instruction {
        program_id: args.program_id,
        accounts: [
            merkle_distributor::accounts::NewClaim {
                distributor,
                from: get_associated_token_address(&distributor, &args.mint),
                to: claimant_ata,
                claimant,
                token_program: token::ID,
            }
            .to_account_metas(None),
            packed_account_metas,
        ]
        .concat(),
        data: merkle_distributor::instruction::NewClaim {
            amount_unlocked: node.amount_unlocked(),
            amount_locked: node.amount_locked(),
            proof: node.proof.expect("proof not found"),
            validity_proof: proof.proof,
            address_tree_info,
            output_state_tree_index,
        }
        .data(),
    };

    ixs.push(new_claim_ix);

    let blockhash = client.get_latest_blockhash().await.unwrap().0;
    let tx =
        Transaction::new_signed_with_payer(&ixs, Some(&claimant.key()), &[&keypair], blockhash);

    match client.client.send_and_confirm_transaction_with_spinner(&tx) {
        Ok(signature) => {
            println!("Created new claim: {signature}");
        }
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("insufficient funds") {
                let token_vault = get_associated_token_address(&distributor, &args.mint);
                eprintln!("Error: Token vault has insufficient funds.");
                eprintln!("  Vault address: {token_vault}");
                eprintln!("  Mint tokens to the vault before claiming:");
                eprintln!("  spl-token mint {} <amount> {}", args.mint, token_vault);
            } else {
                eprintln!("Error creating claim: {e}");
            }
            std::process::exit(1);
        }
    }
}

async fn process_claim(args: &Args, claim_args: &ClaimArgs) {
    let keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");
    let claimant = keypair.pubkey();

    let priority_fee = args.priority.unwrap_or(0);

    let (distributor, _bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);

    let (claim_status_address, _) = get_claim_status_pda(
        &args.program_id,
        &claimant,
        &distributor,
    );

    let photon_url = args.photon_url.clone().unwrap_or_else(|| args.rpc_url.clone());
    let config = LightClientConfig {
        url: args.rpc_url.to_string(),
        photon_url: Some(photon_url),
        commitment_config: None,
        fetch_active_tree: false,
        api_key: None,
    };
    let mut client = LightClient::new(config).await.expect("failed to create client");

    let claim_status_compressed_account = match client
        .get_compressed_account(claim_status_address, None)
        .await
    {
        Ok(response) => match response.value {
            Some(compressed_account) => compressed_account,
            None => {
                println!("PDA does not exist. creating.");
                process_new_claim(args, claim_args).await;
                // Wait a bit for indexer to catch up
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                client
                    .get_compressed_account(claim_status_address, None)
                    .await
                    .expect("Fetching account failed.")
                    .value
                    .expect("Account still not found after creation")
            }
        },
        Err(e) => {
            panic!("error getting PDA: {e}")
        }
    };

    let claim_status = ClaimStatus::deserialize(
        &mut claim_status_compressed_account
            .data
            .as_ref()
            .unwrap()
            .data
            .as_slice(),
    )
    .expect("Claim status compressed account data deserialization failed");

    let validity_proof = client
        .get_validity_proof(
            vec![claim_status_compressed_account.hash],
            vec![],
            None,
        )
        .await
        .expect("get validity proof failed")
        .value;

    // Build v2 PackedStateTreeInfo from the compressed account merkle context
    let mut packed_accounts = PackedAccounts::default();
    packed_accounts.add_system_accounts_v2(SystemAccountMetaConfig::new(merkle_distributor::ID))
        .expect("add system accounts");

    // Add state tree and queue to packed accounts
    let merkle_tree_index = packed_accounts.insert_or_get(claim_status_compressed_account.tree_info.tree);
    let queue_index = packed_accounts.insert_or_get(claim_status_compressed_account.tree_info.queue);

    let tree_info = PackedStateTreeInfo {
        root_index: validity_proof.accounts[0].root_index.root_index().unwrap_or_default(),
        prove_by_index: validity_proof.accounts[0].root_index.proof_by_index(),
        merkle_tree_pubkey_index: merkle_tree_index,
        queue_pubkey_index: queue_index,
        leaf_index: claim_status_compressed_account.leaf_index,
    };

    let input_account_meta = CompressedAccountMeta {
        tree_info,
        address: claim_status_address,
        output_state_tree_index: queue_index,
    };

    let claimant_ata = get_associated_token_address(&claimant, &args.mint);

    let mut ixs = vec![ComputeBudgetInstruction::set_compute_unit_limit(CLAIM_LOCKED_COMPUTE_UNITS)];

    let (packed_account_metas, _, _) = packed_accounts.to_account_metas();

    let claim_ix = Instruction {
        program_id: args.program_id,
        accounts: [
            merkle_distributor::accounts::ClaimLocked {
                distributor,
                from: get_associated_token_address(&distributor, &args.mint),
                to: claimant_ata,
                claimant,
                token_program: token::ID,
            }
            .to_account_metas(None),
            packed_account_metas,
        ]
        .concat(),
        data: merkle_distributor::instruction::ClaimLocked {
            claim_status_data: ClaimStatusInstructionData {
                locked_amount: claim_status.locked_amount,
                locked_amount_withdrawn: claim_status.locked_amount_withdrawn,
                unlocked_amount: claim_status.unlocked_amount,
            },
            validity_proof: validity_proof.proof,
            input_account_meta,
        }
        .data(),
    };
    ixs.push(claim_ix);

    if priority_fee > 0 {
        let instruction = ComputeBudgetInstruction::set_compute_unit_price(priority_fee);
        ixs.push(instruction);
        println!(
            "Added priority fee instruction of {} microlamports",
            priority_fee
        );
    } else {
        println!("No priority fee added. Add one with --priority <microlamports u64>");
    }

    let (blockhash, _) = client.get_latest_blockhash().await.unwrap();
    let tx =
        Transaction::new_signed_with_payer(&ixs, Some(&claimant.key()), &[&keypair], blockhash);

    match client.client.send_and_confirm_transaction_with_spinner(&tx) {
        Ok(signature) => {
            println!("Claimed tokens: {signature}");
        }
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("insufficient funds") {
                let token_vault = get_associated_token_address(&distributor, &args.mint);
                eprintln!("Error: Token vault has insufficient funds.");
                eprintln!("  Vault address: {token_vault}");
                eprintln!("  Mint tokens to the vault before claiming:");
                eprintln!("  spl-token mint {} <amount> {}", args.mint, token_vault);
            } else {
                eprintln!("Error claiming tokens: {e}");
            }
            std::process::exit(1);
        }
    }
}

fn check_distributor_onchain_matches(
    account: &Account,
    merkle_tree: &AirdropMerkleTree,
    new_distributor_args: &NewDistributorArgs,
    pubkey: Pubkey,
) -> Result<(), &'static str> {
    if let Ok(distributor) = MerkleDistributor::try_deserialize(&mut account.data.as_slice()) {
        if distributor.root != merkle_tree.merkle_root {
            return Err("root mismatch");
        }
        if distributor.max_total_claim != merkle_tree.max_total_claim {
            return Err("max_total_claim mismatch");
        }
        if distributor.max_num_nodes != merkle_tree.max_num_nodes {
            return Err("max_num_nodes mismatch");
        }

        if distributor.start_ts != new_distributor_args.start_vesting_ts {
            return Err("start_ts mismatch");
        }
        if distributor.end_ts != new_distributor_args.end_vesting_ts {
            return Err("end_ts mismatch");
        }
        if distributor.clawback_start_ts != new_distributor_args.clawback_start_ts {
            return Err("clawback_start_ts mismatch");
        }
        if distributor.clawback_receiver != new_distributor_args.clawback_receiver_token_account {
            return Err("clawback_receiver mismatch");
        }
        if distributor.admin != pubkey {
            return Err("admin mismatch");
        }
    }
    Ok(())
}

fn process_new_distributor(args: &Args, new_distributor_args: &NewDistributorArgs) {
    let client = RpcClient::new_with_commitment(&args.rpc_url, CommitmentConfig::finalized());

    let keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");
    let merkle_tree = AirdropMerkleTree::new_from_file(&new_distributor_args.merkle_tree_path)
        .expect("failed to read");
    let (distributor_pubkey, _bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);
    let token_vault = get_associated_token_address(&distributor_pubkey, &args.mint);

    if let Some(account) = client
        .get_account_with_commitment(&distributor_pubkey, CommitmentConfig::confirmed())
        .unwrap()
        .value
    {
        println!("merkle distributor account exists, checking parameters...");
        check_distributor_onchain_matches(
            &account,
            &merkle_tree,
            new_distributor_args,
            keypair.pubkey(),
        ).expect("merkle root on-chain does not match provided arguments! Confirm admin and clawback parameters to avoid loss of funds!");
    }

    println!("creating new distributor with args: {new_distributor_args:#?}");

    let new_distributor_ix = Instruction {
        program_id: args.program_id,
        accounts: merkle_distributor::accounts::NewDistributor {
            clawback_receiver: new_distributor_args.clawback_receiver_token_account,
            mint: args.mint,
            token_vault,
            distributor: distributor_pubkey,
            system_program: solana_program::system_program::id(),
            associated_token_program: spl_associated_token_account::ID,
            token_program: token::ID,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: merkle_distributor::instruction::NewDistributor {
            version: args.airdrop_version,
            root: merkle_tree.merkle_root,
            max_total_claim: merkle_tree.max_total_claim,
            max_num_nodes: merkle_tree.max_num_nodes,
            start_vesting_ts: new_distributor_args.start_vesting_ts,
            end_vesting_ts: new_distributor_args.end_vesting_ts,
            clawback_start_ts: new_distributor_args.clawback_start_ts,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[new_distributor_ix],
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    // See comments on new_distributor instruction inside the program to ensure this transaction
    // didn't get frontrun.
    // If this fails, make sure to run it again.
    match client.send_and_confirm_transaction_with_spinner(&tx) {
        Ok(sig) => {
            println!("\nDistributor created: {sig}");
            println!("  Distributor: {distributor_pubkey}");
            println!("  Token vault: {token_vault}");
            println!("\nNext step: mint tokens to the vault:");
            println!("  spl-token mint {} {} {}", args.mint, merkle_tree.max_total_claim, token_vault);
        }
        Err(e) => {
            println!("Failed to create MerkleDistributor: {:?}", e);

            // double check someone didn't frontrun this transaction with a malicious merkle root
            if let Some(account) = client
                .get_account_with_commitment(&distributor_pubkey, CommitmentConfig::processed())
                .unwrap()
                .value
            {
                check_distributor_onchain_matches(
                    &account,
                    &merkle_tree,
                    new_distributor_args,
                    keypair.pubkey(),
                ).expect("merkle root on-chain does not match provided arguments! Confirm admin and clawback parameters to avoid loss of funds!");
            }
        }
    }
}

fn process_clawback(args: &Args, clawback_args: &ClawbackArgs) {
    let payer_keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");
    let clawback_keypair = read_keypair_file(&clawback_args.clawback_keypair_path)
        .expect("Failed reading keypair file");

    let clawback_ata = get_associated_token_address(&clawback_keypair.pubkey(), &args.mint);

    let client = RpcClient::new_with_commitment(&args.rpc_url, CommitmentConfig::confirmed());

    let (distributor, _bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);

    let from = get_associated_token_address(&distributor, &args.mint);
    println!("from: {from}");

    let clawback_ix = Instruction {
        program_id: args.program_id,
        accounts: merkle_distributor::accounts::Clawback {
            distributor,
            from,
            to: clawback_ata,
            claimant: clawback_keypair.pubkey(),
            system_program: solana_program::system_program::ID,
            token_program: token::ID,
        }
        .to_account_metas(None),
        data: merkle_distributor::instruction::Clawback {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[clawback_ix],
        Some(&payer_keypair.pubkey()),
        &[&payer_keypair, &clawback_keypair],
        client.get_latest_blockhash().unwrap(),
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&tx)
        .unwrap();

    println!("Successfully clawed back funds! signature: {signature:#?}");
}

fn process_create_merkle_tree(merkle_tree_args: &CreateMerkleTreeArgs) {
    let merkle_tree = AirdropMerkleTree::new_from_csv(&merkle_tree_args.csv_path).unwrap();
    merkle_tree.write_to_file(&merkle_tree_args.merkle_tree_path);
}

fn process_set_admin(args: &Args, set_admin_args: &SetAdminArgs) {
    let keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");

    let client = RpcClient::new_with_commitment(&args.rpc_url, CommitmentConfig::confirmed());

    let (distributor, _bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);

    let set_admin_ix = Instruction {
        program_id: args.program_id,
        accounts: merkle_distributor::accounts::SetAdmin {
            distributor,
            admin: keypair.pubkey(),
            new_admin: set_admin_args.new_admin,
        }
        .to_account_metas(None),
        data: merkle_distributor::instruction::SetAdmin {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_admin_ix],
        Some(&keypair.pubkey()),
        &[&keypair],
        client.get_latest_blockhash().unwrap(),
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&tx)
        .unwrap();

    println!("Successfully set admin! signature: {signature:#?}");
}
