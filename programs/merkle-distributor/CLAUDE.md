# Index to Merkle Distributor with Compressed PDAs

Distributes SPL tokens via Merkle root. ClaimStatus accounts are compressed to reduce claim costs by ~40x.

## [Guide to get started](../README.md)

## Source Structure

```text
src/
├── lib.rs           # declare_id!, LIGHT_CPI_SIGNER, program module
├── error.rs         # ErrorCode enum (18 variants)
├── state/
│   ├── mod.rs
│   ├── merkle_distributor.rs
│   ├── claim_status.rs       # LightDiscriminator derive
│   └── claimed_event.rs      # NewClaimEvent, ClaimedEvent
└── instructions/
    ├── mod.rs
    ├── new_distributor.rs
    ├── new_claim.rs
    ├── claim_locked.rs
    ├── clawback.rs
    ├── set_admin.rs
    └── set_clawback_receiver.rs
```


## Accounts

### MerkleDistributor (PDA)

Seeds: `["MerkleDistributor", mint.key(), version.to_le_bytes()]`

| Field | Type | Description |
|-------|------|-------------|
| bump | u8 | PDA bump seed |
| version | u64 | Airdrop version |
| root | [u8; 32] | 256-bit Merkle root |
| mint | Pubkey | Token mint to distribute |
| token_vault | Pubkey | ATA holding tokens |
| max_total_claim | u64 | Maximum total claimable tokens |
| max_num_nodes | u64 | Maximum number of claimants |
| total_amount_claimed | u64 | Running total claimed |
| num_nodes_claimed | u64 | Count of unique claimants |
| start_ts | i64 | Vesting start timestamp |
| end_ts | i64 | Vesting end timestamp |
| clawback_start_ts | i64 | Earliest clawback timestamp |
| clawback_receiver | Pubkey | Receives clawback funds |
| admin | Pubkey | Can set admin/clawback receiver |
| clawed_back | bool | Whether funds were clawed back |

### ClaimStatus (Compressed Account)

Address seeds: `["ClaimStatus", claimant.key(), distributor.key()]`

Derives address via `light_sdk::address::v2::derive_address` with `ADDRESS_TREE_V2` (Poseidon hash).

| Property | Value |
|----------|-------|
| Discriminator | 8 bytes (LightDiscriminator derive) |
| Data size | 56 bytes |
| Total serialized | 64 bytes |

| Field | Type | Size | Description |
|-------|------|------|-------------|
| claimant | Pubkey | 32 | Wallet that claimed |
| locked_amount | u64 | 8 | Total locked allocation |
| locked_amount_withdrawn | u64 | 8 | Amount withdrawn so far |
| unlocked_amount | u64 | 8 | Immediately available amount |

## Instructions

| Instruction | Path | Accounts | Logic |
|-------------|------|----------|-------|
| new_distributor | instructions/new_distributor.rs | distributor (init), clawback_receiver, mint, token_vault (init), admin (signer) | Validates timestamps, initializes PDA and vault ATA |
| new_claim | instructions/new_claim.rs | distributor, from (vault), to, claimant (signer) + Light remaining accounts | Verifies Merkle proof, creates compressed ClaimStatus, transfers unlocked_amount |
| claim_locked | instructions/claim_locked.rs | distributor, from (vault), to, claimant (signer) + Light remaining accounts | Calculates vested amount, updates compressed ClaimStatus, transfers tokens |
| clawback | instructions/clawback.rs | distributor, from (vault), to (clawback_receiver), claimant (signer) | Checks clawback_start_ts elapsed, transfers remaining vault balance |
| set_admin | instructions/set_admin.rs | distributor, admin (signer), new_admin | Admin-only, updates distributor.admin |
| set_clawback_receiver | instructions/set_clawback_receiver.rs | distributor, admin (signer), new_clawback_receiver | Admin-only, updates distributor.clawback_receiver |


## Key Concepts

**Vesting**: Linear unlock from `start_ts` to `end_ts`. Formula: `(time_into_unlock * locked_amount) / total_unlock_time`

**Clawback**: Must be ≥1 day after `end_ts`. Anyone can trigger after `clawback_start_ts`.

**Merkle Proof**: `hashv([LEAF_PREFIX, hashv([claimant, amount_unlocked, amount_locked])])` where `LEAF_PREFIX = [0]`

**Light SDK v2**: Uses `derive_address` with `ADDRESS_TREE_V2` constant. CPI via `LightSystemProgramCpi::new_cpi`.

## Security

**Frontrunning risk**: `new_distributor` can be frontrun. Attacker could replace admin/clawback_receiver with their own, set minimal clawback delay, then steal funds. Always verify on-chain state after transaction succeeds.
