use anchor_lang::{
    context::Context, prelude::*, solana_program::hash::hashv, Accounts, Key, Result,
};
use anchor_spl::token_interface::TokenAccount;
use anchor_spl::{token, token::Token};

use jito_merkle_verify::verify;
use light_sdk::{account::LightAccount, NewAddressParamsPacked, ValidityProof};

use crate::{
    error::ErrorCode,
    state::{
        claim_status::ClaimStatus, claimed_event::NewClaimEvent,
        merkle_distributor::MerkleDistributor,
    },
};

// We need to discern between leaf and intermediate nodes to prevent trivial second
// pre-image attacks.
// https://flawed.net.nz/2018/02/21/attacking-merkle-trees-with-a-second-preimage-attack
const LEAF_PREFIX: &[u8] = &[0];

/// [merkle_distributor::new_claim] accounts.
#[derive(Accounts)]
pub struct NewClaim<'info> {
    /// The [MerkleDistributor].
    #[account(mut)]
    pub distributor: Account<'info, MerkleDistributor>,

    /// Distributor ATA containing the tokens to distribute.
    #[account(
        mut,
        associated_token::mint = distributor.mint,
        associated_token::authority = distributor.key(),
        address = distributor.token_vault
    )]
    pub from: InterfaceAccount<'info, TokenAccount>,

    /// Account to send the claimed tokens to.
    #[account(
        mut,
        token::mint=distributor.mint,
        token::authority = claimant.key()
    )]
    pub to: InterfaceAccount<'info, TokenAccount>,

    /// Who is claiming the tokens.
    #[account(mut, address = to.owner @ ErrorCode::OwnerMismatch)]
    pub claimant: Signer<'info>,

    /// SPL [Token] program.
    pub token_program: Program<'info, Token>,
}

/// Initializes a new claim from the [MerkleDistributor].
/// 1. Increments num_nodes_claimed by 1
/// 2. Initializes claim_status
/// 3. Transfers claim_status.unlocked_amount to the claimant
/// 4. Increments total_amount_claimed by claim_status.unlocked_amount
///
/// CHECK:
///     1. The claim window has not expired and the distributor has not been clawed back
///     2. The claimant is the owner of the to account
///     3. Num nodes claimed is less than max_num_nodes
///     4. The merkle proof is valid
#[allow(clippy::result_large_err)]
pub fn handle_new_claim<'info>(
    ctx: Context<'_, '_, '_, 'info, NewClaim<'info>>,
    amount_unlocked: u64,
    amount_locked: u64,
    proof: Vec<[u8; 32]>,
    validity_proof: ValidityProof,
    address_merkle_tree_root_index: u16,
) -> Result<()> {
    let distributor = &mut ctx.accounts.distributor;

    let curr_ts = Clock::get()?.unix_timestamp;
    require!(!distributor.clawed_back, ErrorCode::ClaimExpired);

    distributor.num_nodes_claimed = distributor
        .num_nodes_claimed
        .checked_add(1)
        .ok_or(ErrorCode::ArithmeticError)?;

    require!(
        distributor.num_nodes_claimed <= distributor.max_num_nodes,
        ErrorCode::MaxNodesExceeded
    );

    let claimant_account = &ctx.accounts.claimant;

    // Verify the merkle proof.
    let node = hashv(&[
        &claimant_account.key().to_bytes(),
        &amount_unlocked.to_le_bytes(),
        &amount_locked.to_le_bytes(),
    ]);

    let distributor = &ctx.accounts.distributor;
    let node = hashv(&[LEAF_PREFIX, &node.to_bytes()]);

    require!(
        verify(proof, distributor.root, node.to_bytes()),
        ErrorCode::InvalidProof
    );

    let address_seeds = [
        b"ClaimStatus".as_ref(),
        &ctx.accounts.claimant.key().to_bytes(),
        &ctx.accounts.distributor.key().to_bytes(),
    ];
    let merkle_tree_pubkey = ctx.remaining_accounts[8].key;
    let (address, address_seed) =
        light_sdk::address::v1::derive_address(&address_seeds, merkle_tree_pubkey, &crate::ID);
    let new_address_params = NewAddressParamsPacked {
        seed: address_seed,
        address_merkle_tree_account_index: 0,
        address_queue_account_index: 1,
        address_merkle_tree_root_index,
    };
    let program_id = crate::ID.into();
    let output_merkle_tree_index = 2;
    let mut claim_status = LightAccount::<'_, ClaimStatus>::new_init(
        &program_id,
        Some(address),
        output_merkle_tree_index,
    );
    claim_status.claimant = ctx.accounts.claimant.key();
    claim_status.locked_amount = amount_locked;
    claim_status.unlocked_amount = amount_unlocked;
    claim_status.locked_amount_withdrawn = 0;

    let cpi_inputs = light_sdk::cpi::CpiInputs::new_with_address(
        validity_proof,
        vec![claim_status.to_account_info().unwrap()],
        vec![new_address_params],
    );
    let fee_payer = ctx.accounts.claimant.to_account_info();
    let cpi_accounts =
        light_sdk::cpi::CpiAccounts::new(&fee_payer, ctx.remaining_accounts, crate::ID).unwrap();

    cpi_inputs
        .invoke_light_system_program(cpi_accounts)
        .unwrap();

    let seeds = [
        b"MerkleDistributor".as_ref(),
        &distributor.mint.to_bytes(),
        &distributor.version.to_le_bytes(),
        &[ctx.accounts.distributor.bump],
    ];

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.from.to_account_info(),
                to: ctx.accounts.to.to_account_info(),
                authority: ctx.accounts.distributor.to_account_info(),
            },
        )
        .with_signer(&[&seeds[..]]),
        amount_unlocked,
    )?;

    let distributor = &mut ctx.accounts.distributor;
    distributor.total_amount_claimed = distributor
        .total_amount_claimed
        .checked_add(amount_unlocked)
        .ok_or(ErrorCode::ArithmeticError)?;

    require!(
        distributor.total_amount_claimed <= distributor.max_total_claim,
        ErrorCode::ExceededMaxClaim
    );

    // Note: might get truncated, do not rely on
    msg!(
        "Created new claim with locked {} and {} unlocked with lockup start:{} end:{}",
        amount_locked,
        amount_unlocked,
        distributor.start_ts,
        distributor.end_ts,
    );
    emit!(NewClaimEvent {
        claimant: claimant_account.key(),
        timestamp: curr_ts
    });

    Ok(())
}
