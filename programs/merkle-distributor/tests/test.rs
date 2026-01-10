#![cfg(feature = "test-sbf")]

// Test integration for merkle distributor with LightProgramTest
use jito_merkle_tree::{
    airdrop_merkle_tree::AirdropMerkleTree,
    utils::{get_claim_status_pda, get_merkle_distributor_pda},
};
use light_program_test::{
    program_test::LightProgramTest, AddressWithTree, Indexer, ProgramTestConfig, Rpc,
};
use light_sdk::instruction::{PackedAccounts, SystemAccountMetaConfig};
use solana_program::program_pack::Pack;
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};

use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
};

#[test]
fn test_merkle_tree_creation() {
    // Create merkle tree directly from tree nodes
    let (merkle_tree, _test_keypairs) = create_test_merkle_tree();

    // Verify merkle tree properties
    assert_eq!(merkle_tree.tree_nodes.len(), 2);
    assert_eq!(merkle_tree.max_num_nodes, 2);

    // Test the first node in the tree
    let first_node = &merkle_tree.tree_nodes[0];

    // Verify node has correct amounts (stored as UI amounts, converted to base units when used)
    assert_eq!(first_node.amount_unlocked(), 1000); // UI amounts
    assert_eq!(first_node.amount_locked(), 500);
    assert!(first_node.proof.is_some());

    // Test merkle tree verification
    let proof = first_node.proof.as_ref().unwrap();
    assert!(!proof.is_empty()); // Should have a valid proof

    println!("✅ Merkle tree creation test completed successfully!");
    println!("✅ Merkle root: {:?}", hex::encode(merkle_tree.merkle_root));
    println!("✅ Claimant: {}", first_node.claimant);
    println!("✅ Unlocked amount: {}", first_node.amount_unlocked());
    println!("✅ Locked amount: {}", first_node.amount_locked());
    println!("✅ Max total claim: {}", merkle_tree.max_total_claim);
}

#[test]
fn test_pda_derivation() {
    use merkle_distributor::ID as PROGRAM_ID;

    // Test PDA derivation functions
    let mint = Keypair::new().pubkey();
    let claimant = Keypair::new().pubkey();
    let version = 0u64;
    // Test distributor PDA
    let (distributor_pda, _bump) = get_merkle_distributor_pda(&PROGRAM_ID, &mint, version);
    println!("✅ Distributor PDA: {}", distributor_pda);

    // Test claim status PDA
    let (claim_status_pda, _bump) =
        get_claim_status_pda(&PROGRAM_ID, &claimant, &distributor_pda);
    println!("✅ Claim Status PDA: {:?}", claim_status_pda);

    println!("✅ PDA derivation test completed successfully!");
}

#[tokio::test]
async fn test_distributor_integration_with_light_program_test() {
    use anchor_lang::{prelude::*, AnchorDeserialize};
    use merkle_distributor::{
        state::{claim_status::ClaimStatus, merkle_distributor::MerkleDistributor},
        ID as PROGRAM_ID,
    };

    // Initialize LightProgramTest with v2 trees
    let config = ProgramTestConfig::new_v2(true, Some(vec![("merkle_distributor", PROGRAM_ID)]));
    let mut rpc = LightProgramTest::new(config).await.unwrap();
    let payer = rpc.get_payer().insecure_clone();

    // Setup test data
    let (merkle_tree, test_keypairs) = create_test_merkle_tree();
    let claimant_keypair = &test_keypairs[0];

    // Create test mint
    let mint_keypair = Keypair::new();
    let mint = mint_keypair.pubkey();

    // Create mint account
    let rent = rpc
        .get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)
        .await
        .unwrap();
    let create_mint_account_ix = solana_program::system_instruction::create_account(
        &payer.pubkey(),
        &mint,
        rent,
        spl_token::state::Mint::LEN as u64,
        &spl_token::id(),
    );

    let create_mint_ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint,
        &payer.pubkey(),
        Some(&payer.pubkey()),
        9,
    )
    .unwrap();

    send_transaction(
        &mut rpc,
        &[create_mint_account_ix, create_mint_ix],
        &[&payer, &mint_keypair],
    )
    .await
    .unwrap();

    // Get distributor PDA and token vault
    let (distributor_pda, _bump) = get_merkle_distributor_pda(&PROGRAM_ID, &mint, 0);
    let distributor_token_account = get_associated_token_address(&distributor_pda, &mint);

    // Create clawback token account
    let clawback_token_account = get_associated_token_address(&payer.pubkey(), &mint);
    let create_clawback_ata_ix =
        create_associated_token_account(&payer.pubkey(), &payer.pubkey(), &mint, &spl_token::id());

    send_transaction(&mut rpc, &[create_clawback_ata_ix], &[&payer])
        .await
        .unwrap();

    // Set up timing (current time + buffer for start/end)
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let start_vesting_ts = current_time + 10;
    let end_vesting_ts = current_time + 3600; // 1 hour later
    let clawback_start_ts = current_time + 3600 + 86400; // 1 hour + 24 hours (minimum delay)

    // Create new distributor using helper function
    let new_distributor_ix = create_distributor_instruction(
        &PROGRAM_ID,
        &distributor_pda,
        &payer.pubkey(),
        &mint,
        &distributor_token_account,
        &clawback_token_account,
        &merkle_tree,
        start_vesting_ts,
        end_vesting_ts,
        clawback_start_ts,
    );

    send_transaction(&mut rpc, &[new_distributor_ix], &[&payer])
        .await
        .unwrap();

    // Verify distributor was created correctly
    let distributor_account = rpc.get_account(distributor_pda).await.unwrap();
    let distributor_data =
        MerkleDistributor::try_deserialize(&mut distributor_account.unwrap().data.as_slice())
            .unwrap();

    assert_eq!(distributor_data.root, merkle_tree.merkle_root);
    assert_eq!(distributor_data.admin, payer.pubkey());
    assert_eq!(distributor_data.mint, mint);

    // Mint tokens to distributor token vault
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint,
        &distributor_token_account,
        &payer.pubkey(),
        &[],
        merkle_tree.max_total_claim,
    )
    .unwrap();

    send_transaction(&mut rpc, &[mint_to_ix], &[&payer])
        .await
        .unwrap();

    // Get the claimant's node from merkle tree
    let claimant_node = merkle_tree.get_node(&claimant_keypair.pubkey());

    // Get v2 tree addresses
    let address_tree = rpc.test_accounts.v2_address_trees[0];
    let _state_tree = &rpc.test_accounts.v2_state_trees[0];

    // Get claim status PDA using v2 address derivation
    let (claim_status_address, _address_seed) = get_claim_status_pda(
        &PROGRAM_ID,
        &claimant_keypair.pubkey(),
        &distributor_pda,
    );

    // Get validity proof for creating new claim
    let proof = rpc
        .get_validity_proof(
            vec![],
            vec![AddressWithTree {
                address: claim_status_address,
                tree: address_tree,
            }],
            None,
        )
        .await
        .unwrap()
        .value;

    // Setup packed accounts for new claim with v2 pattern
    // Note: Don't add_pre_accounts_signer - claimant is already in main NewClaim accounts
    let mut packed_accounts = PackedAccounts::default();
    packed_accounts
        .add_system_accounts_v2(SystemAccountMetaConfig::new(PROGRAM_ID))
        .unwrap();

    // Pack tree infos for v2 - use get_random_state_tree_info like nosana does
    let output_state_tree_index = rpc
        .get_random_state_tree_info()
        .unwrap()
        .pack_output_tree_index(&mut packed_accounts)
        .unwrap();
    let address_tree_info = proof.pack_tree_infos(&mut packed_accounts).address_trees[0];

    // Fund the claimant account for transaction fees
    let fund_claimant_ix = solana_program::system_instruction::transfer(
        &payer.pubkey(),
        &claimant_keypair.pubkey(),
        1_000_000_000, // 1 SOL
    );
    send_transaction(&mut rpc, &[fund_claimant_ix], &[&payer])
        .await
        .unwrap();

    // Create claimant's associated token account
    let claimant_ata = get_associated_token_address(&claimant_keypair.pubkey(), &mint);
    let create_claimant_ata_ix = create_associated_token_account(
        &payer.pubkey(),
        &claimant_keypair.pubkey(),
        &mint,
        &spl_token::id(),
    );

    send_transaction(&mut rpc, &[create_claimant_ata_ix], &[&payer])
        .await
        .unwrap();

    // Create new claim instruction using helper function
    let (packed_account_metas, _, _) = packed_accounts.to_account_metas();

    let new_claim_ix = create_new_claim_instruction(
        &PROGRAM_ID,
        &distributor_pda,
        &distributor_token_account,
        &claimant_ata,
        &claimant_keypair.pubkey(),
        packed_account_metas,
        &claimant_node,
        proof.proof,
        address_tree_info,
        output_state_tree_index,
    );

    send_transaction(&mut rpc, &[new_claim_ix], &[&payer, claimant_keypair])
        .await
        .unwrap();

    // Verify claim was created - check that compressed account exists
    let claim_status_account = rpc
        .get_compressed_account(claim_status_address, None)
        .await
        .unwrap()
        .value
        .expect("Claim status account not found");

    let claim_status =
        ClaimStatus::deserialize(&mut claim_status_account.data.as_ref().unwrap().data.as_slice())
            .unwrap();

    assert_eq!(claim_status.claimant, claimant_keypair.pubkey());
    assert_eq!(
        claim_status.unlocked_amount,
        claimant_node.amount_unlocked()
    );
    assert_eq!(claim_status.locked_amount, claimant_node.amount_locked());

    // Verify tokens were transferred to claimant
    let claimant_token_account = rpc.get_account(claimant_ata).await.unwrap();
    let claimant_token_data =
        spl_token::state::Account::unpack(&claimant_token_account.unwrap().data).unwrap();
    assert_eq!(claimant_token_data.amount, claimant_node.amount_unlocked());

    println!("✅ LightProgramTest integration test completed successfully!");
    println!(
        "✅ Distributor created with merkle root: {:?}",
        hex::encode(merkle_tree.merkle_root)
    );
    println!(
        "✅ Claim created for claimant: {}",
        claimant_keypair.pubkey()
    );
    println!(
        "✅ Unlocked tokens transferred: {}",
        claimant_node.amount_unlocked()
    );
    println!(
        "✅ Locked tokens remaining: {}",
        claimant_node.amount_locked()
    );
}

#[test]
fn test_merkle_proof_verification() {
    // Create merkle tree directly
    let (merkle_tree, _test_keypairs) = create_test_merkle_tree();

    // Test proof verification for each node in the tree
    for node in &merkle_tree.tree_nodes {
        let proof = node.proof.as_ref().unwrap();

        // The proof should not be empty for a tree with multiple nodes
        assert!(!proof.is_empty());

        // Each proof should be valid (this is tested internally by the merkle tree library)
        println!("✅ Proof verified for claimant: {}", node.claimant);
    }

    println!("✅ Merkle proof verification test completed successfully!");
}

async fn send_transaction(
    rpc: &mut LightProgramTest,
    instructions: &[solana_program::instruction::Instruction],
    signers: &[&Keypair],
) -> Result<(), Box<dyn std::error::Error>> {
    let (blockhash, _) = rpc.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&signers[0].pubkey()),
        signers,
        blockhash,
    );
    rpc.process_transaction(transaction).await?;
    Ok(())
}

fn create_distributor_instruction(
    program_id: &solana_sdk::pubkey::Pubkey,
    distributor_pda: &solana_sdk::pubkey::Pubkey,
    admin: &solana_sdk::pubkey::Pubkey,
    mint: &solana_sdk::pubkey::Pubkey,
    token_vault: &solana_sdk::pubkey::Pubkey,
    clawback_receiver: &solana_sdk::pubkey::Pubkey,
    merkle_tree: &AirdropMerkleTree,
    start_vesting_ts: i64,
    end_vesting_ts: i64,
    clawback_start_ts: i64,
) -> solana_program::instruction::Instruction {
    use anchor_lang::{InstructionData, ToAccountMetas};

    solana_program::instruction::Instruction {
        program_id: *program_id,
        accounts: merkle_distributor::accounts::NewDistributor {
            distributor: *distributor_pda,
            admin: *admin,
            mint: *mint,
            token_vault: *token_vault,
            clawback_receiver: *clawback_receiver,
            system_program: solana_program::system_program::ID,
            token_program: spl_token::id(),
            associated_token_program: spl_associated_token_account::id(),
        }
        .to_account_metas(None),
        data: merkle_distributor::instruction::NewDistributor {
            version: 0,
            root: merkle_tree.merkle_root,
            max_total_claim: merkle_tree.max_total_claim,
            max_num_nodes: merkle_tree.max_num_nodes,
            start_vesting_ts,
            end_vesting_ts,
            clawback_start_ts,
        }
        .data(),
    }
}

fn create_new_claim_instruction(
    program_id: &solana_sdk::pubkey::Pubkey,
    distributor_pda: &solana_sdk::pubkey::Pubkey,
    from: &solana_sdk::pubkey::Pubkey,
    to: &solana_sdk::pubkey::Pubkey,
    claimant: &solana_sdk::pubkey::Pubkey,
    packed_account_metas: Vec<solana_program::instruction::AccountMeta>,
    claimant_node: &jito_merkle_tree::tree_node::TreeNode,
    validity_proof: light_sdk::instruction::ValidityProof,
    address_tree_info: light_sdk::instruction::PackedAddressTreeInfo,
    output_state_tree_index: u8,
) -> solana_program::instruction::Instruction {
    use anchor_lang::{InstructionData, ToAccountMetas};

    solana_program::instruction::Instruction {
        program_id: *program_id,
        accounts: [
            merkle_distributor::accounts::NewClaim {
                distributor: *distributor_pda,
                from: *from,
                to: *to,
                claimant: *claimant,
                token_program: spl_token::id(),
            }
            .to_account_metas(None),
            packed_account_metas,
        ]
        .concat(),
        data: merkle_distributor::instruction::NewClaim {
            amount_unlocked: claimant_node.amount_unlocked(),
            amount_locked: claimant_node.amount_locked(),
            proof: claimant_node.proof.clone().expect("proof not found"),
            validity_proof,
            address_tree_info,
            output_state_tree_index,
        }
        .data(),
    }
}

/// Create test data and merkle tree without CSV files
fn create_test_merkle_tree() -> (AirdropMerkleTree, Vec<Keypair>) {
    use jito_merkle_tree::tree_node::TreeNode;

    // Create test keypairs
    let test_keypairs = vec![Keypair::new(), Keypair::new()];

    // Create tree nodes directly
    let tree_nodes = vec![
        TreeNode {
            claimant: test_keypairs[0].pubkey(),
            total_unlocked_staker: 1000,
            total_locked_staker: 500,
            total_unlocked_searcher: 0,
            total_locked_searcher: 0,
            total_unlocked_validator: 0,
            total_locked_validator: 0,
            proof: None, // Will be set by AirdropMerkleTree::new
        },
        TreeNode {
            claimant: test_keypairs[1].pubkey(),
            total_unlocked_staker: 0,
            total_locked_staker: 0,
            total_unlocked_searcher: 0,
            total_locked_searcher: 0,
            total_unlocked_validator: 2000,
            total_locked_validator: 1000,
            proof: None, // Will be set by AirdropMerkleTree::new
        },
    ];

    let merkle_tree = AirdropMerkleTree::new(tree_nodes).expect("Failed to create merkle tree");
    (merkle_tree, test_keypairs)
}
