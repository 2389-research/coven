// ABOUTME: Scenario 8 - Test fixed fold-client (no nested runtime)
// ABOUTME: This should work now that block_on is removed

use fold_client::FoldClient;

#[tokio::main]
async fn main() {
    println!("=== Scenario 8: Fixed FoldClient (async-safe) ===");
    println!();

    // Test 1: Local gateway
    println!("Test 1: Local gateway (port 50051)");
    test_client("http://localhost:50051").await;
    println!();

    // Test 2: Tailscale gateway (within tailnet)
    println!("Test 2: Tailscale gateway (within tailnet, port 50051)");
    test_client("http://100.100.78.27:50051").await;
    println!();

    // Test 3: Nested async call (the previous failure case)
    println!("Test 3: Nested async - calling from spawned task");
    let handle = tokio::spawn(async {
        test_client("http://localhost:50051").await;
    });
    match handle.await {
        Ok(()) => println!("  Nested spawn completed without panic!"),
        Err(e) => println!("  FAIL: Spawn panicked: {}", e),
    }
}

async fn test_client(url: &str) {
    println!("  URL: {}", url);

    let client = FoldClient::new(url.to_string());

    match client.check_health().await {
        Ok(()) => {
            println!("  PASS: check_health succeeded");
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            if err_str.contains("Unauthenticated") {
                println!("  PASS: Connection works (Unauthenticated as expected)");
            } else if err_str.contains("Connection") {
                println!("  SKIP: Gateway not reachable ({})", err_str);
            } else {
                println!("  FAIL: {}", err_str);
            }
        }
    }
}
