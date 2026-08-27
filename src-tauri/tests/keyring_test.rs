//! Test keyring read/write on Windows to diagnose #72
//! Run with: cargo test -p dby --test keyring_test -- --nocapture

use keyring::Entry;

#[test]
#[ignore = "requires manual inspection of Windows Credential Manager"]
fn test_keyring_roundtrip() {
    let test_key = "dby_test_key_roundtrip";
    let test_value = "test_password_123";
    
    // Clean up any existing entry
    let _ = Entry::new("dby", test_key)
        .and_then(|e| e.delete_credential());
    
    println!("=== Testing keyring roundtrip ===");
    
    // Write
    println!("Writing password to keyring...");
    let entry = Entry::new("dby", test_key).expect("Failed to create entry");
    entry.set_password(test_value).expect("Failed to set password");
    println!("✓ Password written successfully");
    
    // Try reading with the SAME entry instance
    println!("Reading password back with same entry instance...");
    match entry.get_password() {
        Ok(retrieved) => {
            println!("✓ Password retrieved with same instance: {}", retrieved);
            assert_eq!(retrieved, test_value, "Password mismatch with same instance!");
        }
        Err(e) => {
            println!("✗ Failed to read with same instance: {:?}", e);
        }
    }
    
    // Try reading with a NEW entry instance (simulating reconnect scenario)
    println!("Reading password back with NEW entry instance...");
    let entry2 = Entry::new("dby", test_key).expect("Failed to create entry for read");
    match entry2.get_password() {
        Ok(retrieved) => {
            println!("✓ Password retrieved with new instance: {}", retrieved);
            assert_eq!(retrieved, test_value, "Password mismatch with new instance!");
        }
        Err(keyring::Error::NoEntry) => {
            println!("✗ NoEntry error with new instance - THIS IS THE BUG!");
            panic!("Windows keyring bug: password was written but cannot be read back with new Entry instance");
        }
        Err(e) => {
            println!("✗ Other error with new instance: {:?}", e);
            panic!("Unexpected error: {:?}", e);
        }
    }
    
    // Clean up
    println!("Cleaning up...");
    entry2.delete_credential().expect("Failed to delete");
    println!("✓ Test completed successfully");
    
    println!("\nPlease check Windows Credential Manager to verify the entry was created and deleted.");
}

#[test]
#[ignore = "requires manual inspection"]
fn test_keyring_read_nonexistent() {
    println!("=== Testing read of non-existent key ===");
    let entry = Entry::new("dby", "nonexistent_key_12345").expect("Failed to create entry");
    match entry.get_password() {
        Ok(pw) => panic!("Should not have found password, got: {}", pw),
        Err(keyring::Error::NoEntry) => {
            println!("✓ Correctly returned NoEntry error");
        }
        Err(e) => {
            println!("✗ Got unexpected error: {:?}", e);
            panic!("Unexpected error: {:?}", e);
        }
    }
}
