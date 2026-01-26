// ABOUTME: Scenario 7 - Test gRPC on Tailscale with correct port (50051)
// ABOUTME: gRPC is NOT exposed via Funnel, only within tailnet

use fold_grpc_client::{create_channel, ChannelConfig};
use fold_proto::fold::client_service_client::ClientServiceClient;

#[tokio::main]
async fn main() {
    println!("=== Scenario 7: Tailscale gRPC on port 50051 ===");
    println!();
    println!("NOTE: gRPC is only accessible within the tailnet, not via public Funnel");
    println!();

    // Test 1: Funnel URL (443) - HTTP works, gRPC won't
    println!("Test 1: Funnel URL (port 443 implied) - expect fail for gRPC");
    test_direct("https://fold-gateway.porpoise-alkaline.ts.net").await;
    println!();

    // Test 2: Explicit gRPC port within tailnet
    println!("Test 2: Tailnet URL with gRPC port 50051");
    test_direct("https://fold-gateway.porpoise-alkaline.ts.net:50051").await;
    println!();

    // Test 3: Try without TLS (in case tailnet doesn't use TLS for internal)
    println!("Test 3: Tailnet URL without TLS (http) port 50051");
    test_direct("http://fold-gateway.porpoise-alkaline.ts.net:50051").await;
    println!();

    // Test 4: Using tailscale IP directly if we know it
    println!("Test 4: If you're on tailnet, try the Tailscale IP directly");
    println!("        (run 'tailscale status' to find the gateway IP)");
}

async fn test_direct(url: &str) {
    println!("  URL: {}", url);

    let config = ChannelConfig::new(url);

    match create_channel(&config).await {
        Ok(channel) => {
            println!("  Channel created");

            let mut client = ClientServiceClient::new(channel);

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
