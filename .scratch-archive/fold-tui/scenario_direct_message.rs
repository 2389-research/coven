// ABOUTME: Direct gRPC message test bypassing fold-client
// ABOUTME: Tests raw gRPC send_message + stream_events to isolate issues

use fold_grpc_client::{create_channel, ChannelConfig};
use fold_proto::fold::client_service_client::ClientServiceClient;
use fold_proto::fold::{ClientSendMessageRequest, StreamEventsRequest, ListAgentsRequest};
use fold_ssh::{load_key, SshAuthCredentials};
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tonic::Request;

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║        DIRECT gRPC MESSAGE TEST                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let gateway_url = "http://fold-gateway.porpoise-alkaline.ts.net:50051";
    let ssh_key_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/fold/agent_key");

    if !ssh_key_path.exists() {
        println!("❌ SSH key not found at {:?}", ssh_key_path);
        return;
    }

    println!("Loading SSH key...");
    let key = match load_key(&ssh_key_path) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            println!("❌ Failed to load SSH key: {:?}", e);
            return;
        }
    };
    println!("✅ SSH key loaded");
    println!();

    println!("Creating channel to {}...", gateway_url);
    let config = ChannelConfig::new(gateway_url);
    let channel = match create_channel(&config).await {
        Ok(c) => c,
        Err(e) => {
            println!("❌ Channel creation failed: {:?}", e);
            return;
        }
    };
    println!("✅ Channel created");
    println!();

    // Create SSH auth credentials
    let creds = match SshAuthCredentials::new(&key) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            println!("❌ Failed to create credentials: {:?}", e);
            return;
        }
    };

    // Create client with SSH interceptor
    let creds_clone = creds.clone();
    let mut client = ClientServiceClient::with_interceptor(channel, move |mut req: Request<()>| {
        creds_clone.apply_to_request(&mut req)
            .map_err(|e| tonic::Status::internal(format!("Auth failed: {:?}", e)))?;
        Ok(req)
    });

    // First list agents to find target
    println!("Listing agents...");
    match client.list_agents(ListAgentsRequest { workspace: None }).await {
        Ok(response) => {
            let agents = response.into_inner().agents;
            println!("Found {} agent(s):", agents.len());
            for a in &agents {
                let status = if a.connected { "🟢" } else { "🔴" };
                println!("  {} {} (backend: {})", status, a.name, a.backend);
            }
            println!();

            // Find aster-fold-agent
            let target = agents
                .iter()
                .find(|a| a.name.contains("aster") && a.connected);

            if let Some(agent) = target {
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("SENDING MESSAGE");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("  Agent: {} ({})", agent.name, agent.id);
                println!();

                // Send message
                let send_request = ClientSendMessageRequest {
                    conversation_key: agent.id.clone(),
                    content: "Respond with exactly: PONG".to_string(),
                    attachments: vec![],
                    idempotency_key: format!("test-{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()),
                };

                println!("  Sending: \"{}\"", send_request.content);
                match client.send_message(send_request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        println!("  ✅ Message sent!");
                        println!("     Status: {:?}", resp.status);
                        println!("     Message ID: {}", resp.message_id);
                        println!();

                        // Now stream events
                        println!("  Opening event stream...");
                        let stream_request = StreamEventsRequest {
                            conversation_key: agent.id.clone(),
                            since_event_id: None,
                        };

                        match client.stream_events(stream_request).await {
                            Ok(response) => {
                                println!("  ✅ Stream opened, waiting for events...");
                                println!();

                                let mut stream = response.into_inner();
                                let timeout = Duration::from_secs(60);
                                let start = std::time::Instant::now();

                                while start.elapsed() < timeout {
                                    match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                                        Ok(Some(Ok(event))) => {
                                            println!("  [EVENT] {:?}", event);
                                            if let Some(payload) = &event.payload {
                                                match payload {
                                                    fold_proto::fold::client_stream_event::Payload::Done(_) => {
                                                        println!();
                                                        println!("  ✅ DONE event received!");
                                                        return;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Ok(Some(Err(e))) => {
                                            println!("  ❌ Stream error: {:?}", e);
                                            return;
                                        }
                                        Ok(None) => {
                                            println!("  Stream ended");
                                            return;
                                        }
                                        Err(_) => {
                                            print!(".");
                                            std::io::Write::flush(&mut std::io::stdout()).ok();
                                        }
                                    }
                                }
                                println!();
                                println!("  ⚠️  Timeout waiting for events");
                            }
                            Err(e) => {
                                println!("  ❌ Failed to open stream: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("  ❌ Send failed: {} ({})", e.message(), e.code());
                    }
                }
            } else {
                println!("⚠️  No connected aster agent found");
            }
        }
        Err(e) => {
            println!("❌ Failed to list agents: {:?}", e);
        }
    }
}
