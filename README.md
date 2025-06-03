# merkle-distributor

A program for distributing tokens efficiently via uploading a [Merkle root](https://en.wikipedia.org/wiki/Merkle_tree).

Based on jito airdrop Merkle distributor.
User ClaimStatus accounts are compressed accounts -> the claim process is significantly cheaper for users.


## Test

cargo test-sbf


## CLI

**PreRequisites**
-- solana cli
-- spl-token-cli  `$ cargo install spl-token-cli`
-- light cli `$ npm -g i @lightprotocol/zk-compression-cli`


0. build: `anchor build && cargo build -p jito-scripts`
1. Setup env: `light test-validator --sbf-program mERKcfxMC5SqJn4Ld4BUris3WKZZ1ojjWJ3A3J5CKxv ./target/deploy/merkle_distributor.so`
2. create mint `spl-token create-token`
3. create test csv
3. create Merkle tree from csv `./target/debug/cli --mint <mint_address> --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899  create-merkle-tree  --csv-path ./merkle-tree/test_fixtures/test_csv.csv  --merkle-tree-path ./merkle_tree.json`
    - test data is in merkle-tree/test_fixtures/test_csv.csv
4. create clawback token account `spl-token create-account <mint_address>`
5. setup new distributor `./target/debug/cli --mint <mint_address> --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 new-distributor --clawback-receiver-token-account   <address> --start-vesting-ts 1748484386 --end-vesting-ts 1748484387 --merkle-tree-path ./merkle_tree.json --clawback-start-ts  1848484387`
    - --start-vesting-ts are unix timestamps
6. mint to distributor token pool `spl-token mint <mint_address> <amount> <token_account>`
7. claim for each test participant `./target/debug/cli --mint <mint_address> --keypair-path ~/.config/solana/id.json --rpc-url http://localhost:8899 claim --merkle-tree-path ./merkle_tree.json`


TODO:
1. add create-test-csv command that creates csv with 1 recipient from solana address
2. add get-time-stamp command with optional test flag test flag prints unix ts in 30s
3. print distributor account when creating new distributor
4. figure out why the claim cli command does two instructions instead of 1


## Disclaimer

This is a proof of concept implementation, not audited and not ready for production use.
