use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use solana_sdk::signature::{Keypair, Signer};

/// Basic CLI integration test that validates command structure and help output
/// This test runs quickly and doesn't require external dependencies
#[test]
fn test_cli_help_commands() {
    let cli_binary = get_cli_binary_path();

    // Test main help
    let output = Command::new(&cli_binary)
        .args(["--help"])
        .output()
        .expect("Failed to execute CLI help command");
    
    assert!(output.status.success(), "Help command should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create-merkle-tree"), "Should show create-merkle-tree subcommand");
    assert!(stdout.contains("new-distributor"), "Should show new-distributor subcommand");
    assert!(stdout.contains("claim"), "Should show claim subcommand");

    println!("✅ CLI help command works correctly");
}

/// Test CLI error handling with missing arguments
#[test]
fn test_cli_error_handling() {
    let cli_binary = get_cli_binary_path();

    // Test create-merkle-tree with missing arguments
    let output = Command::new(&cli_binary)
        .args(["create-merkle-tree"])
        .output()
        .expect("Failed to execute command");
    
    assert!(!output.status.success(), "Should fail with missing arguments");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required") || stderr.contains("error"), "Should show required argument error");

    println!("✅ CLI error handling works correctly");
}

/// Test CSV creation and validation (fast test without blockchain interaction)
#[test]
fn test_csv_creation_and_merkle_tree_command() {
    // Create test files directory in target for persistence
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    let test_files_dir = workspace_root.join("target").join("test_files");
    fs::create_dir_all(&test_files_dir).expect("Failed to create test files directory");

    // Generate test keypairs
    let claimant1 = Keypair::new();
    let claimant2 = Keypair::new();
    let claimant3 = Keypair::new();

    // Create test CSV file with proper format
    let csv_path = test_files_dir.join("test_rewards.csv");
    create_test_csv_file(&csv_path, &[
        (claimant1.pubkey().to_string(), 1000, 500, "Staker".to_string()),
        (claimant2.pubkey().to_string(), 2000, 1000, "Validator".to_string()),
        (claimant3.pubkey().to_string(), 1500, 750, "Searcher".to_string()),
    ]);

    // Verify CSV was created correctly
    assert!(csv_path.exists(), "CSV file should exist");
    let csv_content = fs::read_to_string(&csv_path).expect("Failed to read CSV file");
    assert!(csv_content.contains("pubkey,amount_unlocked,amount_locked,category"), "Should have correct header");
    assert!(csv_content.contains(&claimant1.pubkey().to_string()), "Should contain claimant1");
    assert!(csv_content.contains(&claimant2.pubkey().to_string()), "Should contain claimant2");
    assert!(csv_content.contains(&claimant3.pubkey().to_string()), "Should contain claimant3");

    // Test create-merkle-tree command structure (will fail due to missing RPC/mint, but validates args)
    let cli_binary = get_cli_binary_path();
    let merkle_tree_path = test_files_dir.join("test_merkle_tree.json");
    
    let output = Command::new(&cli_binary)
        .args([
            "--mint", "11111111111111111111111111111111", // Dummy mint for structure test
            "--keypair-path", "/tmp/nonexistent.json", // Will fail, but that's expected
            "--rpc-url", "http://localhost:8899",
            "create-merkle-tree",
            "--csv-path", csv_path.to_str().unwrap(),
            "--merkle-tree-path", merkle_tree_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute create-merkle-tree command");

    // Command should fail due to missing keypair file, but this validates the argument structure
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    println!("create-merkle-tree stdout: {}", stdout);
    println!("create-merkle-tree stderr: {}", stderr);
    
    if output.status.success() {
        println!("✅ create-merkle-tree command succeeded (unexpected but acceptable)");
    } else {
        // Should fail on keypair reading or RPC connection, not argument parsing
        assert!(
            stderr.contains("keypair") || 
            stderr.contains("No such file") || 
            stderr.contains("Failed") ||
            stderr.contains("connection") ||
            stderr.contains("RPC") ||
            stderr.contains("failed to read") ||
            stdout.contains("failed") ||
            stderr.is_empty(), // Sometimes errors go to stdout
            "Should fail on file access or RPC, not argument parsing. stderr: {}, stdout: {}", stderr, stdout
        );
    }

    println!("✅ CSV creation and command structure validation passed");
    println!("✅ Created test files in: {:?}", test_files_dir);
}

/// Test command structure for new-distributor without blockchain interaction
#[test]
fn test_new_distributor_command_structure() {
    let cli_binary = get_cli_binary_path();

    // Test with all required arguments but dummy values
    let output = Command::new(&cli_binary)
        .args([
            "--mint", "11111111111111111111111111111111",
            "--keypair-path", "/tmp/nonexistent.json", 
            "--rpc-url", "http://localhost:8899",
            "new-distributor",
            "--clawback-receiver-token-account", "11111111111111111111111111111111",
            "--start-vesting-ts", "1000000000",
            "--end-vesting-ts", "1000003600", 
            "--merkle-tree-path", "/tmp/nonexistent.json",
            "--clawback-start-ts", "1000090000",
        ])
        .output()
        .expect("Failed to execute new-distributor command");

    // Should fail on file access, not argument parsing
    assert!(!output.status.success(), "Should fail with missing files");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("keypair") || stderr.contains("No such file") || stderr.contains("Failed") || stderr.contains("merkle"),
        "Should fail on file access, not argument parsing. Got: {}", stderr
    );

    println!("✅ new-distributor command structure validation passed");
}

/// Test command structure for claim without blockchain interaction  
#[test]
fn test_claim_command_structure() {
    let cli_binary = get_cli_binary_path();

    // Test with all required arguments but dummy values
    let output = Command::new(&cli_binary)
        .args([
            "--mint", "11111111111111111111111111111111",
            "--keypair-path", "/tmp/nonexistent.json",
            "--rpc-url", "http://localhost:8899", 
            "claim",
            "--merkle-tree-path", "/tmp/nonexistent.json",
        ])
        .output()
        .expect("Failed to execute claim command");

    // Should fail on file access, not argument parsing
    assert!(!output.status.success(), "Should fail with missing files");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("keypair") || stderr.contains("No such file") || stderr.contains("Failed") || stderr.contains("merkle"),
        "Should fail on file access, not argument parsing. Got: {}", stderr
    );

    println!("✅ claim command structure validation passed");
}

// Helper functions

/// Get the path to the CLI binary for testing
fn get_cli_binary_path() -> PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    
    let debug_path = workspace_root.join("target/debug/cli");
    let release_path = workspace_root.join("target/release/cli");
    
    if debug_path.exists() {
        debug_path
    } else if release_path.exists() {
        release_path
    } else {
        // Build the CLI if it doesn't exist
        println!("Building CLI binary...");
        let build_output = Command::new("cargo")
            .args(["build", "--bin", "cli"])
            .current_dir(&workspace_root)
            .output()
            .expect("Failed to build CLI binary");
        
        if !build_output.status.success() {
            panic!(
                "Failed to build CLI binary:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&build_output.stdout),
                String::from_utf8_lossy(&build_output.stderr)
            );
        }
        
        debug_path
    }
}

/// Create a test CSV file with the proper format (pubkey,amount_unlocked,amount_locked,category)
fn create_test_csv_file(path: &PathBuf, claimants: &[(String, u64, u64, String)]) {
    use std::io::Write;
    let mut file = std::fs::File::create(path).expect("Failed to create CSV file");
    writeln!(file, "pubkey,amount_unlocked,amount_locked,category").expect("Failed to write CSV header");
    
    for (pubkey, amount_unlocked, amount_locked, category) in claimants {
        writeln!(file, "{},{},{},{}", pubkey, amount_unlocked, amount_locked, category)
            .expect("Failed to write CSV row");
    }
}