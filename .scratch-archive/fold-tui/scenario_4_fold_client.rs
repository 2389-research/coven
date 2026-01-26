// ABOUTME: Scenario 4 - Test fold-client library directly
// ABOUTME: This is what the TUI and iOS apps use

// Run with: cargo run --manifest-path .scratch/Cargo.toml

use fold_client::FoldClient;

fn main() {
    println!("=== Scenario 4: fold-client Library Test ===");
    println!("Testing: FoldClient::check_health() against localhost:50051");
    println!();

    // Test 1: Wrong port (what TUI was using)
    println!("Test 1: Connect to port 5000 (TUI default - WRONG)");
    test_connection("http://localhost:5000");
    println!();

    // Test 2: Correct port
    println!("Test 2: Connect to port 50051 (actual gateway port)");
    test_connection("http://localhost:50051");
    println!();

    // Test 3: HTTP port (should fail for gRPC)
    println!("Test 3: Connect to port 8080 (HTTP port - should fail for gRPC)");
    test_connection("http://localhost:8080");
}

fn test_connection(url: &str) {
    println!("  URL: {}", url);

    let client = FoldClient::new(url.to_string());

    // Use catch_unwind to handle panics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            client.check_health().await
        })
    }));

    match result {
        Ok(Ok(())) => println!("  ✅ PASS: Connection successful"),
        Ok(Err(e)) => println!("  ❌ FAIL: {}", e),
        Err(_) => println!("  ❌ PANIC: Runtime or connection panicked"),
    }
}
