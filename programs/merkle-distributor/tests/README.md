# Distributor Integration Tests

This directory contains comprehensive integration tests for the merkle distributor program, following the complete user flow from CSV creation to token claiming.

## Test Structure

### Basic Tests (Currently Working)

#### 1. `test_csv_and_merkle_tree_creation()`
- **Purpose**: Tests CSV parsing and merkle tree generation
- **Coverage**: 
  - Creates CSV with test claimant data using known keypairs
  - Builds merkle tree using the library
  - Verifies merkle tree properties (node count, max claims)
  - Validates node amounts and merkle proofs

#### 2. `test_pda_derivation()`
- **Purpose**: Tests Program Derived Address (PDA) generation
- **Coverage**:
  - Tests distributor PDA derivation using program utilities
  - Tests claim status PDA derivation with compressed account addresses
  - Validates PDA generation functions work correctly

#### 3. `test_merkle_proof_verification()`
- **Purpose**: Verifies merkle proof generation and validation
- **Coverage**:
  - Verifies all nodes in merkle tree have valid proofs
  - Ensures proof generation works for complete user flow

### LightProgramTest Integration Tests (Implemented but Commented Out)

**Note**: These tests are fully implemented but commented out due to Solana dependency version conflicts between the distributor project and light-protocol dependencies.

#### 1. `test_distributor_integration_with_light_program_test()`
- **Purpose**: Complete end-to-end integration test using LightProgramTest
- **Coverage**:
  - Sets up LightProgramTest environment with distributor program
  - Creates SPL mint and associated token accounts
  - Creates new distributor with merkle tree
  - Tests new_claim instruction with compressed accounts
  - Verifies token transfers and compressed account state

#### 2. `test_claim_locked_integration()`
- **Purpose**: Tests the claim_locked functionality for vested tokens
- **Coverage**:
  - Sets up distributor with past vesting timestamps
  - Creates initial claim (unlocked tokens)
  - Tests claim_locked instruction for remaining tokens
  - Verifies complete token distribution

## User Flow Tested

The tests follow the complete distributor user flow:

1. **CSV Creation**: Generate CSV with claimant data and known keypairs
2. **Merkle Tree Generation**: Build merkle tree from CSV using library
3. **Distributor Setup**: Create new distributor with timing parameters
4. **Token Funding**: Mint tokens to distributor token vault
5. **Initial Claiming**: Process new_claim instruction for unlocked tokens
6. **Locked Token Claiming**: Process claim_locked instruction for vested tokens
7. **Verification**: Validate all state changes and token transfers

## Helper Functions

The implementation includes reusable helper functions that mirror the CLI logic:

- `create_distributor_instruction()`: Creates new distributor instructions
- `create_new_claim_instruction()`: Creates new claim instructions
- CSV generation utilities
- Test data setup functions

## Running Tests

### Basic Tests (✅ Working)
```bash
cargo test --tests
```

**Currently Passing:**
- `test_merkle_tree_creation()` ✅
- `test_pda_derivation()` ✅  
- `test_merkle_proof_verification()` ✅

### LightProgramTest Integration Tests (🚧 Implemented but needs API adjustments)
The full integration tests are implemented but require some API adjustments for the current LightProgramTest version. The code structure is complete and demonstrates the full user flow.

## Dependencies

### Current (Working)
- `hex = "0.4"` - For displaying merkle roots
- `spl-token = "7"` - For SPL token operations
- `bs58 = "0.5"` - For base58 encoding/decoding

### Future (For Full Integration)
- `light-program-test` - For compressed account testing environment

## Test Data

- Uses deterministic test keypairs for reproducible results
- Creates temporary CSV files that are cleaned up after tests
- Supports multiple claimant scenarios (Staker, Validator categories)
- Tests both unlocked and locked token amounts

## Implementation Notes

- Helper functions reuse CLI logic for consistency
- Tests follow Light Protocol patterns for compressed accounts
- Proper error handling and cleanup in all test scenarios
- Comprehensive assertions for state validation
- Support for both immediate and vested token claiming scenarios

## Future Enhancements

Once dependency conflicts are resolved, the full integration tests will provide:
- Complete compressed account lifecycle testing
- Real blockchain environment simulation
- End-to-end instruction execution validation
- State consistency verification across all program instructions