# merkle-distributor

A program for distributing tokens efficiently via uploading a [Merkle root](https://en.wikipedia.org/wiki/Merkle_tree).

## Claiming Airdrop via CLI

To claim via CLI run the following commands.

1. Build the cli (must have rust + cargo installed):

```bash
cargo b -r
```

2. Run `claim` with the proper args. Be sure to replace `<YOUR KEYPAIR>` with the _full path_ of your keypair file. This will transfer tokens from the account `8Xm3tkQH581s3MoRHWUNYA5jKbgPATW4tJAAxgwDC6T6` to a the associated token account owned by your keypair, creating it if it doesn't exist.

```bash
./target/release/cli --rpc-url https://api.mainnet-beta.solana.com --keypair-path <YOUR KEYPAIR> --airdrop-version 0 --mint jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL --program-id mERKcfxMC5SqJn4Ld4BUris3WKZZ1ojjWJ3A3J5CKxv claim --merkle-tree-path merkle_tree.json
```

Note that for searchers and validators, not all tokens will be vested until December 7, 2024. You can check the vesting status at `https://jito.network/airdrop`.

## Test

**PreRequisites**
-- solana cli
-- spl-token-cli  `$ cargo install spl-token-cli`
-- light cli `$ npm -g i @lightprotocol/zk-compression-cli`

TODO:
1. add create-test-csv command that creates csv with 1 recipient from solana address
2. add get-time-stamp command with optional test flag test flag prints unix ts in 30s
3. print distributor account when creating new distributor
4. figure out why the claim cli command does two instructions instead of 1

0. build: `anchor build && cargo build -p jito-scripts`
1. Setup env: `light test-validator --sbf-program mERKcfxMC5SqJn4Ld4BUris3WKZZ1ojjWJ3A3J5CKxv ./target/deploy/merkle_distributor.so`
2. create mint `spl-token create-token`
3. create test csv
3. create Merkle tree from csv `./target/debug/cli --mint <mint_address> --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899  create-merkle-tree  --csv-path ./merkle-tree/test_fixtures/test_csv.csv  --merkle-tree-path ./merkle_tree.json`
    - test data is in merkle-tree/test_fixtures/test_csv.csv
4. create clawback token account `spl-token create-account <mint_address>`
5. setup new distributor `./target/debug/cli --mint 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 new-distributor --clawback-receiver-token-account  CVw3m7kEoJCCPAMU59oooJ4PvPzaSb6pC5LC1VANzJ6F --start-vesting-ts 1748484386 --end-vesting-ts 1748484387 --merkle-tree-path ./merkle_tree.json --clawback-start-ts  1848484387`
    - --start-vesting-ts are unix timestamps
6. mint to distributor token pool `spl-token mint <mint_address> <amount> <token_account>`
7. claim for each test participant `./target/debug/cli --mint 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 claim --merkle-tree-path ./merkle_tree.json`

mint: 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L
clawback token account: CVw3m7kEoJCCPAMU59oooJ4PvPzaSb6pC5LC1VANzJ6F
spl-token create-account 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L
spl-token mint 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L 1000000000 P52LTT5xQSXFYjk9Yu25xxw4b2EafccgPfpyAFqMdxg

./target/debug/cli --mint 5bxbmRfJGpfPRNvHesV9rNTWfMhoFAU2vJDMuNj4xM8L --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899  create-merkle-tree  --csv-path ./merkle-tree/test_fixtures/test_csv.csv  --merkle-tree-path ./merkle_tree.json

Distributor account:
- 3uUrKVoa6MYoYU4AbbEFESR629PMGBUJoqFixezmU8Fq

### Next steps:
- make tree poseidon tree of height 26 -> then we can use our circuits to generate zkps
- batched claim with batched Merkle proofs

for depins:
1. depin signup -> creates (compressed) pda with ID and counter
2. depin device signup -> create compressed pda with address assigned to depin
3. create distribution -> compressed account with root, depin ID, dist ID (dist ID is derived from counter), counter value, increment counter in depin pda
4. claim
  inputs:
  - depin device(s)
  - distributions (in ascending order)
  - Merkle proofs of distributions
  - calculate eligible amount
