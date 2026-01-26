// ABOUTME: Quick test to verify gRPC connection works
// ABOUTME: Run with: cargo run --example test-connect -- <gateway-url>

use coven_grpc::{create_channel, ChannelConfig};
use coven_proto::client::ClientServiceClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:50051".to_string());

    println!("Connecting to: {}", url);

    let config = ChannelConfig::new(&url);
    println!("Creating channel...");

    let channel = create_channel(&config).await?;
    println!("Channel created!");

    let mut client = ClientServiceClient::new(channel);
    println!("Calling GetMe...");

    let response = client.get_me(()).await?;
    println!("Response: {:?}", response);

    Ok(())
}
