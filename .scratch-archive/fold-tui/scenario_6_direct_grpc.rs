// ABOUTME: Scenario 6 - Bypass fold-client, test gRPC directly
// ABOUTME: If this works, the issue is in fold-client, not gRPC layer

use fold_grpc_client::{create_channel, ChannelConfig};
use fold_proto::fold::client_service_client::ClientServiceClient;

#[tokio::main]
async fn main() {
    println!("=== Scenario 6: Direct gRPC (bypass fold-client) ===");
    println!();

    // Test against real Tailscale gateway
    println!("Test 1: Real Tailscale gateway");
    test_direct("https://fold-gateway.porpoise-alkaline.ts.net").await;
    println!();

    // Test local
    println!("Test 2: Local gateway port 50051");
    test_direct("http://localhost:50051").await;
    println!();

    // Wrong port
    println!("Test 3: Wrong port 5000");
    test_direct("http://localhost:5000").await;
}

async fn test_direct(url: &str) {
    println!("  URL: {}", url);

    let config = ChannelConfig::new(url);

    match create_channel(&config).await {
        Ok(channel) => {
            println!("  Channel created");

            let mut client = ClientServiceClient::new(channel);

            // Try GetMe (will fail with Unauthenticated, but connection should work)
            match client.get_me(()).await {
                Ok(response) => {
                    println!("  ✅ PASS: GetMe succeeded: {:?}", response);
                }
                Err(e) => {
                    let status = e.code();
                    if status == tonic::Code::Unauthenticated {
                        println!("  ✅ PASS: Connection works (Unauthenticated as expected)");
                    } else {
                        println!("  ❌ FAIL: gRPC error: {} ({})", e.message(), status);
                    }
                }
            }
        }
        Err(e) => {
            println!("  ❌ FAIL: Channel creation failed: {}", e);
        }
    }
}
