# merkle-distributor

A program to distribute SPL tokens via uploading a [Merkle root](https://en.wikipedia.org/wiki/Merkle_tree).

Based on jito airdrop Merkle distributor and optimized with rent-free PDAs. 

User ClaimStatus accounts are compressed accounts, to reduce cost for the claim process:

| Account type   | Cost per claim | 100k claims |
|----------------|----------------|-------------|
| PDA     | ~0.002 SOL     | ~200 SOL    |
| Compressed PDA | ~0.00005 SOL   | ~5 SOL      |


## Documentation

- [Rationale](programs/merkle-distributor/README.md)
- [AI Assistance Reference](programs/merkle-distributor/CLAUDE.md)
- [Documentation](https://www.zkcompression.com)

## Get Started

### Prerequisites

- solana cli
- spl-token-cli  `cargo install spl-token-cli`
- light cli `npm i -g @lightprotocol/zk-compression-cli`

### 0. Build and Test

```bash
cargo build-sbf && cargo build -p jito-scripts
```

```bash
cargo test-sbf
```

### 1. Start test validator

```bash
light test-validator --sbf-program mERKcfxMC5SqJn4Ld4BUris3WKZZ1ojjWJ3A3J5CKxv ./target/deploy/merkle_distributor.so
```

### 2. Create mint

```bash
MINT=$(spl-token create-token | grep "Address:" | awk '{print $2}')
echo "Mint: $MINT"
```

### 3. Create test CSV with your wallet

```bash
echo "pubkey,amount_unlocked,amount_locked,category
$(solana address),1000,500,Staker" > test_airdrop.csv
```

### 4. Create merkle tree from CSV

```bash
./target/debug/cli --mint $MINT --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 \
  create-merkle-tree --csv-path ./test_airdrop.csv --merkle-tree-path ./merkle_tree.json
```

### 5. Create clawback token account

```bash
CLAWBACK=$(spl-token create-account $MINT | grep "Creating account" | awk '{print $3}')
echo "Clawback: $CLAWBACK"
```

### 6. Create distributor

Timestamps must satisfy: `clawback_start >= end_vesting + 86400` (1 day minimum)

```bash
START_TS=$(($(date +%s) + 10))
END_TS=$(($(date +%s) + 60))
CLAWBACK_TS=$((END_TS + 86400 + 60))

./target/debug/cli --mint $MINT --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 \
  new-distributor \
  --clawback-receiver-token-account $CLAWBACK \
  --start-vesting-ts $START_TS \
  --end-vesting-ts $END_TS \
  --clawback-start-ts $CLAWBACK_TS \
  --merkle-tree-path ./merkle_tree.json
```

### 7. Mint tokens to the vault

The previous step prints the token vault address and the mint command.

```bash
# Use the command printed by new-distributor, e.g.:
spl-token mint $MINT 1500000000000 <TOKEN_VAULT>
```

### 8. Claim tokens

```bash
./target/debug/cli --mint $MINT --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 \
  --photon-url http://localhost:8784 claim --merkle-tree-path ./merkle_tree.json
```

## Disclaimer

This is a proof of concept implementation, not audited and not ready for production use.