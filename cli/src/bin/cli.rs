extern crate jito_merkle_tree;
extern crate merkle_distributor;

use std::{path::PathBuf, str::FromStr};

use anchor_lang::{
    prelude::Pubkey, pubkey, AccountDeserialize, AnchorDeserialize, InstructionData, Key,
    ToAccountMetas,
};
use anchor_spl::token;
use clap::{Parser, Subcommand};
use jito_merkle_tree::{
    airdrop_merkle_tree::AirdropMerkleTree,
    utils::{get_claim_status_pda, get_merkle_distributor_pda},
};
use light_client::{
    indexer::{AddressWithTree, Indexer, StateMerkleTreeAccounts},
    rpc::{rpc_connection::RpcConnectionConfig, RpcConnection, SolanaRpcConnection},
};
use light_sdk::{
    instruction::{
        account_meta::CompressedAccountMeta,
        accounts::SystemAccountMetaConfig,
        merkle_context::{
            pack_address_merkle_context, pack_merkle_context, AddressMerkleContext, MerkleContext,
        },
        pack_accounts::PackedAccounts,
    },
    light_compressed_account::TreeType,
    Poseidon,
};
use merkle_distributor::state::{claim_status::ClaimStatus, merkle_distributor::MerkleDistributor};
use solana_program::instruction::Instruction;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::{
    account::Account, bs58, commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};

pub const V1_ADDRESS_MERKLE_TREE: Pubkey = pubkey!("amt1Ayt45jfbdw5YSo7iz6WZxUmnZsQTYXy82hVwyC2");
pub const V1_ADDRESS_QUEUE: Pubkey = pubkey!("aq1S9z4reTSQAdgWHGD2zDaS39sjGrAxbR31vxJ2F4F");
pub const V1_MERKLE_TREE_PUBKEY: Pubkey = pubkey!("smt1NamzXdq4AMqS2fS2F1i5KTYPZRhoHgWx38d8WsT");

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
        &V1_ADDRESS_MERKLE_TREE,
    );
    println!("address {:?}", claim_status_address);
    println!("_address_seed {:?}", _address_seed);

    let config = RpcConnectionConfig::new(args.rpc_url.to_string());
    let mut client = SolanaRpcConnection::new(config);

    let claimant_ata = get_associated_token_address(&claimant, &args.mint);

    let mut ixs = vec![ComputeBudgetInstruction::set_compute_unit_limit(400_000)];
    let proof = client
        .get_validity_proof(
            vec![],
            vec![AddressWithTree {
                address: claim_status_address,
                tree: V1_ADDRESS_MERKLE_TREE,
            }],
        )
        .await
        .expect("failed to get validity proof");
    let address_merkle_context = AddressMerkleContext {
        address_queue_pubkey: V1_ADDRESS_QUEUE,
        address_merkle_tree_pubkey: V1_ADDRESS_MERKLE_TREE,
    };

    println!("proof {:?}", proof);
    let mut packed_accounts = PackedAccounts::new_with_system_accounts(
        SystemAccountMetaConfig::new(merkle_distributor::ID),
    );
    pack_address_merkle_context(
        &address_merkle_context,
        &mut packed_accounts,
        proof.address_root_indices[0],
    );

    packed_accounts.insert_or_get(V1_MERKLE_TREE_PUBKEY);

    // let account = client
    //     .get_compressed_account(Some(claim_status_address), None)
    //     .await;

    match client.get_account(claimant_ata).await {
        Ok(_) => {}
        Err(e) => {
            // TODO: directly pattern match on error kind
            if e.to_string().contains("AccountNotFound") {
                println!("PDA does not exist. creating.");
                let ix =
                    create_associated_token_account(&claimant, &claimant, &args.mint, &token::ID);
                ixs.push(ix);
            } else {
                panic!("Error fetching PDA: {e}")
            }
        }
    }
    let (packed_account_metas, _, packed_accounts_start_offset) =
        packed_accounts.to_account_metas();
    println!(
        "packed_accounts_start_offset {}",
        packed_accounts_start_offset
    );
    println!(
        "get_associated_token_address(&distributor, &args.mint) {}",
        get_associated_token_address(&distributor, &args.mint)
    );
    let new_claim_ix = Instruction {
        program_id: args.program_id,
        accounts: [
            merkle_distributor::accounts::NewClaim {
                distributor,
                // claim_status: claim_status_pda,
                from: get_associated_token_address(&distributor, &args.mint),
                to: claimant_ata,
                claimant,
                token_program: token::ID,
                // system_program: solana_program::system_program::ID,
            }
            .to_account_metas(None),
            packed_account_metas,
        ]
        .concat(),
        data: merkle_distributor::instruction::NewClaim {
            amount_unlocked: node.amount_unlocked(),
            amount_locked: node.amount_locked(),
            proof: node.proof.expect("proof not found"),
            address_merkle_tree_root_index: proof.address_root_indices[0],
            validity_proof: proof.proof.into(),
        }
        .data(),
    };

    ixs.push(new_claim_ix);

    let blockhash = client.get_latest_blockhash().await.unwrap().0;
    let tx =
        Transaction::new_signed_with_payer(&ixs, Some(&claimant.key()), &[&keypair], blockhash);

    let signature = client
        .client
        .send_and_confirm_transaction_with_spinner(&tx)
        .unwrap();
    println!("successfully created new claim with signature {signature:#?}");
}

async fn process_claim(args: &Args, claim_args: &ClaimArgs) {
    let keypair = read_keypair_file(&args.keypair_path).expect("Failed reading keypair file");
    let claimant = keypair.pubkey();

    let priority_fee = args.priority.unwrap_or(0);

    let (distributor, bump) =
        get_merkle_distributor_pda(&args.program_id, &args.mint, args.airdrop_version);
    println!("distributor pubkey {}", distributor);

    let (claim_status_address, _address_seed) = get_claim_status_pda(
        &args.program_id,
        &claimant,
        &distributor,
        &V1_ADDRESS_MERKLE_TREE,
    );
    println!("claim pda: {claim_status_address:?}, bump: {_address_seed:?}");

    let config = RpcConnectionConfig::new(args.rpc_url.to_string());
    let mut client = SolanaRpcConnection::new(config);

    let claim_status_compressed_account = match client
        .get_compressed_account(Some(claim_status_address), None)
        .await
    {
        Ok(compressed_account) => compressed_account,
        Err(e) => {
            // TODO: match on the error kind
            if e.to_string().contains("Account not found") {
                println!("PDA does not exist. creating.");
                process_new_claim(args, claim_args).await;
            } else {
                panic!("error getting PDA: {e}")
            }
            // TODO: wait for indexer to catch up
            client
                .get_compressed_account(Some(claim_status_address), None)
                .await
                .expect("Fetching account failed.")
        }
    };
    let mut by_owner = client
        .get_compressed_accounts_by_owner(&merkle_distributor::ID)
        .await
        .expect("Fetching account failed.");
    by_owner[0]
        .compressed_account
        .data
        .as_mut()
        .unwrap()
        .discriminator = [144, 240, 8, 28, 159, 72, 157, 125];
    println!("by_owner {by_owner:?}");
    println!(
        "claim_status_compressed_account {:?}",
        claim_status_compressed_account
    );
    let data_hash = bs58::decode(
        claim_status_compressed_account
            .data
            .as_ref()
            .unwrap()
            .data_hash
            .clone(),
    )
    .into_vec()
    .unwrap();
    println!("decoded data_hash {:?}", data_hash);
    let hash = bs58::decode(claim_status_compressed_account.hash.clone())
        .into_vec()
        .unwrap();
    println!("decoded hash {:?}", hash);
    let address = bs58::decode(claim_status_compressed_account.address.as_ref().unwrap())
        .into_vec()
        .unwrap();
    println!("decoded address {:?}", address);
    println!(
        "by owner address {:?}",
        by_owner[0].compressed_account.address
    );
    println!("offchain address {:?}", address);
    println!("by owner {:?}", by_owner[0]);

    println!("by owner hash {:?}", by_owner[0].hash().unwrap());
    println!(
        "by owner hash not batched {:?}",
        by_owner[0]
            .compressed_account
            .hash(
                &by_owner[0].merkle_context.merkle_tree_pubkey,
                &by_owner[0].merkle_context.leaf_index,
                false
            )
            .unwrap()
    );
    println!(
        "by owner hash batched {:?}",
        by_owner[0]
            .compressed_account
            .hash(&V1_MERKLE_TREE_PUBKEY, &1, true)
            .unwrap()
    );
    println!(
        "by owner data hash {:?}",
        by_owner[0]
            .compressed_account
            .data
            .as_ref()
            .unwrap()
            .data_hash
    );

    let claim_status = ClaimStatus::deserialize(
        &mut base64::decode(
            claim_status_compressed_account
                .data
                .as_ref()
                .unwrap()
                .data
                .clone()
                .as_bytes(),
        )
        .expect("Claim status compressed account data deserialization failed")
        .as_slice(),
    )
    .expect("Claim status compressed account data deserialization failed");

    println!("des claim status {:?}", claim_status);

    let claim_status = ClaimStatus::deserialize(
        &mut by_owner[0]
            .compressed_account
            .data
            .as_ref()
            .unwrap()
            .data
            .as_slice(),
    )
    .expect("Claim status compressed account data deserialization failed");
    println!("des claim status {:?}", claim_status);

    let claim_status_compressed_account = client
        .get_compressed_account(None, Some(by_owner[0].hash().unwrap()))
        .await
        .expect("Fetching account failed.");
    println!(
        "claim_status_compressed_account by hash {:?}",
        claim_status_compressed_account
    );

    let validity_proof = client
        .get_validity_proof(
            vec![bs58::decode(claim_status_compressed_account.hash)
                .into_vec()
                .unwrap()
                .try_into()
                .unwrap()],
            vec![],
        )
        .await
        .expect("get validity proof failed");

    let merkle_context = MerkleContext {
        merkle_tree_pubkey: Pubkey::from_str(&claim_status_compressed_account.tree).unwrap(),
        // TODO: add lookup table logic
        queue_pubkey: Pubkey::from_str("nfq1NvQDJ2GEgnS8zt9prAe8rjjpAW1zFkrvZoBR148").unwrap(),
        leaf_index: claim_status_compressed_account.leaf_index,
        tree_type: TreeType::StateV1,
        prove_by_index: false,
    };
    let mut packed_accounts = PackedAccounts::new_with_system_accounts(
        SystemAccountMetaConfig::new(merkle_distributor::ID),
    );
    let merkle_context = pack_merkle_context(&merkle_context, &mut packed_accounts);

    let input_account_meta = CompressedAccountMeta {
        merkle_context,
        address: claim_status_address,
        // TODO: make flexible
        output_merkle_tree_index: merkle_context.merkle_tree_pubkey_index,
        root_index: Some(validity_proof.root_indices[0]),
    };

    let claimant_ata = get_associated_token_address(&claimant, &args.mint);

    let mut ixs = vec![ComputeBudgetInstruction::set_compute_unit_limit(500_000)];

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
            claim_status,
            validity_proof: validity_proof.proof.into(),
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

    let signature = client
        .client
        .send_and_confirm_transaction_with_spinner(&tx)
        .unwrap();
    println!("successfully claimed tokens with signature {signature:#?}",);
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
        Ok(_) => {}
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
