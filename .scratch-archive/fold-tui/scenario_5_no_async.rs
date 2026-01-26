// ABOUTME: Scenario 5 - Test fold-client WITHOUT an outer async runtime
// ABOUTME: This mimics how iOS/Swift FFI would use it (no existing runtime)

// Run with: cargo run --bin scenario_5

use fold_client::FoldClient;

fn main() {
    println!("=== Scenario 5: fold-client WITHOUT outer async runtime ===");
    println!();

    // Test against the real Tailscale gateway
    println!("Test 1: Real Tailscale gateway (HTTPS gRPC)");
    test_sync("https://fold-gateway.porpoise-alkaline.ts.net");
    println!();

    // Test against local with correct port
    println!("Test 2: Local gateway (if running) - port 50051");
    test_sync("http://localhost:50051");
    println!();

    // Test wrong port to see error
    println!("Test 3: Wrong port 5000 (TUI's default)");
    test_sync("http://localhost:5000");
}

fn test_sync(url: &str) {
    println!("  URL: {}", url);

    let client = FoldClient::new(url.to_string());

    // NO outer async runtime - call the async fn using the client's internal runtime
    // This requires using block_on ourselves since check_health is async
    // But FoldClient already does block_on internally, so we can't await it

    // The problem: check_health() is marked async but calls block_on internally
    // We need a sync API, not async-that-blocks

    // For now, let's just see if creating the client works
    println!("  Client created successfully");

    // Try to call refresh_agents which also uses block_on
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Create a new runtime just for this call
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            client.check_health().await
        })
    }));

    match result {
        Ok(Ok(())) => println!("  ✅ PASS: Health check passed"),
        Ok(Err(e)) => println!("  ❌ ERROR: {}", e),
        Err(e) => println!("  ❌ PANIC: {:?}", e.downcast_ref::<&str>()),
    }
}
