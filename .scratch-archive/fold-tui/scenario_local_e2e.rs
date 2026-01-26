// ABOUTME: Local e2e test for client message routing
// ABOUTME: Tests message flow: client → gateway → agent → events back to client

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
    println!("║        LOCAL E2E MESSAGE ROUTING TEST                        ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Local gateway
    let gateway_url = "http://localhost:50051";
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
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("STEP 1: LIST AGENTS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    match client.list_agents(ListAgentsRequest { workspace: None }).await {
        Ok(response) => {
            let agents = response.into_inner().agents;
            println!("Found {} agent(s):", agents.len());
            for a in &agents {
                let status = if a.connected { "🟢" } else { "🔴" };
                println!("  {} {} (backend: {})", status, a.name, a.backend);
            }
            println!();

            // Find the agent with pack tools (ends with -1, has pack tools)
            // Or any connected mux agent as fallback
            let target = agents
                .iter()
                .find(|a| a.connected && a.name.ends_with("-1"))
                .or_else(|| agents.iter().find(|a| a.connected && a.backend == "mux"));

            if let Some(agent) = target {
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("STEP 2: SEND MESSAGE");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("  Target Agent: {} ({})", agent.name, agent.id);
                println!();

                // Send message - test the echo pack tool
                let send_request = ClientSendMessageRequest {
                    conversation_key: agent.id.clone(),
                    content: "Use the echo tool to echo the message 'Hello Pack Tools E2E Test!'".to_string(),
                    attachments: vec![],
                    idempotency_key: format!("local-e2e-{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()),
                };

                println!("  Sending: \"{}\"", send_request.content);
                match client.send_message(send_request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        println!("  ✅ Message accepted by gateway!");
                        println!("     Status: {:?}", resp.status);
                        println!("     Message ID: {}", resp.message_id);
                        println!();

                        // Now stream events
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        println!("STEP 3: STREAM EVENTS");
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        let stream_request = StreamEventsRequest {
                            conversation_key: agent.id.clone(),
                            since_event_id: None,
                        };

                        match client.stream_events(stream_request).await {
                            Ok(response) => {
                                println!("  ✅ Stream opened, waiting for events...");
                                println!();

                                let mut stream = response.into_inner();
                                let timeout = Duration::from_secs(30);
                                let start = std::time::Instant::now();
                                let mut event_count = 0;
                                let mut got_done = false;

                                while start.elapsed() < timeout {
                                    match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
                                        Ok(Some(Ok(event))) => {
                                            event_count += 1;
                                            if let Some(payload) = &event.payload {
                                                match payload {
                                                    fold_proto::fold::client_stream_event::Payload::Event(evt) => {
                                                        println!("  [EVENT] id={}", evt.id);
                                                        if let Some(text) = &evt.text {
                                                            println!("    text: \"{}\"", text);
                                                        }
                                                    }
                                                    fold_proto::fold::client_stream_event::Payload::Thinking(t) => {
                                                        println!("  [THINKING] \"{}\"", t.content);
                                                    }
                                                    fold_proto::fold::client_stream_event::Payload::Text(t) => {
                                                        println!("  [TEXT] \"{}\"", t.content);
                                                    }
                                                    fold_proto::fold::client_stream_event::Payload::Done(_) => {
                                                        println!("  [DONE]");
                                                        got_done = true;
                                                    }
                                                    fold_proto::fold::client_stream_event::Payload::Error(e) => {
                                                        println!("  [ERROR] msg={} recoverable={}", e.message, e.recoverable);
                                                    }
                                                    _ => {
                                                        println!("  [OTHER] {:?}", payload);
                                                    }
                                                }
                                            }
                                            if got_done {
                                                break;
                                            }
                                        }
                                        Ok(Some(Err(e))) => {
                                            println!("  ❌ Stream error: {:?}", e);
                                            break;
                                        }
                                        Ok(None) => {
                                            println!("  Stream ended");
                                            break;
                                        }
                                        Err(_) => {
                                            // Timeout on this poll, continue
                                            print!(".");
                                            std::io::Write::flush(&mut std::io::stdout()).ok();
                                        }
                                    }
                                }

                                println!();
                                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                                println!("RESULT");
                                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                                println!("  Events received: {}", event_count);
                                if got_done && event_count > 0 {
                                    println!("  ✅ E2E TEST PASSED: Message routed through gateway to agent");
                                } else if event_count > 0 {
                                    println!("  ⚠️  PARTIAL: Got {} events but no DONE marker", event_count);
                                } else {
                                    println!("  ❌ E2E TEST FAILED: No events received (routing broken?)");
                                }
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
                println!("⚠️  No connected agent found - start an agent first:");
                println!("    cd fold-agent && cargo run -- --server http://localhost:50051 --name test-agent");
            }
        }
        Err(e) => {
            println!("❌ Failed to list agents: {:?}", e);
        }
    }
}
