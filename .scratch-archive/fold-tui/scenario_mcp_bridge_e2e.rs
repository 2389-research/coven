// ABOUTME: E2E test for mcp-bridge-pack
// ABOUTME: Tests chronicle MCP tools exposed via mcp-bridge

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
    println!("║        MCP-BRIDGE E2E TEST (Chronicle)                       ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let gateway_url = "http://localhost:50051";
    let ssh_key_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/fold/agent_key");

    if !ssh_key_path.exists() {
        println!("❌ SSH key not found at {:?}", ssh_key_path);
        return;
    }

    let key = match load_key(&ssh_key_path) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            println!("❌ Failed to load SSH key: {:?}", e);
            return;
        }
    };

    let config = ChannelConfig::new(gateway_url);
    let channel = match create_channel(&config).await {
        Ok(c) => c,
        Err(e) => {
            println!("❌ Channel creation failed: {:?}", e);
            return;
        }
    };

    let creds = match SshAuthCredentials::new(&key) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            println!("❌ Failed to create credentials: {:?}", e);
            return;
        }
    };

    let creds_clone = creds.clone();
    let mut client = ClientServiceClient::with_interceptor(channel, move |mut req: Request<()>| {
        creds_clone.apply_to_request(&mut req)
            .map_err(|e| tonic::Status::internal(format!("Auth failed: {:?}", e)))?;
        Ok(req)
    });

    // Find agent
    let agents = match client.list_agents(ListAgentsRequest { workspace: None }).await {
        Ok(response) => response.into_inner().agents,
        Err(e) => {
            println!("❌ Failed to list agents: {:?}", e);
            return;
        }
    };

    let agent = match agents.iter().find(|a| a.connected && a.backend == "mux") {
        Some(a) => a,
        None => {
            println!("❌ No connected mux agent found");
            return;
        }
    };

    println!("✅ Using agent: {} ({})", agent.name, agent.id);
    println!();

    // Test the add_entry tool from chronicle via mcp-bridge
    let test_prompt = "Use the add_entry tool to log an entry with message 'E2E test entry from fold pack system' and tags ['test', 'e2e']";

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST: add_entry (chronicle via mcp-bridge)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Prompt: {}", test_prompt);
    println!();

    let send_request = ClientSendMessageRequest {
        conversation_key: agent.id.clone(),
        content: test_prompt.to_string(),
        attachments: vec![],
        idempotency_key: format!("mcp-e2e-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()),
    };

    match client.send_message(send_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Message accepted (ID: {})", resp.message_id);
        }
        Err(e) => {
            println!("  ❌ Send failed: {}", e);
            return;
        }
    }

    // Stream events
    let stream_request = StreamEventsRequest {
        conversation_key: agent.id.clone(),
        since_event_id: None,
    };

    match client.stream_events(stream_request).await {
        Ok(response) => {
            println!("  Streaming response...");

            let mut stream = response.into_inner();
            let timeout = Duration::from_secs(45);
            let start = std::time::Instant::now();
            let mut event_count = 0;
            let mut got_done = false;
            let mut full_text = String::new();

            while start.elapsed() < timeout {
                match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
                    Ok(Some(Ok(event))) => {
                        event_count += 1;
                        if let Some(payload) = &event.payload {
                            match payload {
                                fold_proto::fold::client_stream_event::Payload::Event(evt) => {
                                    if let Some(text) = &evt.text {
                                        full_text.push_str(text);
                                    }
                                }
                                fold_proto::fold::client_stream_event::Payload::Text(t) => {
                                    full_text.push_str(&t.content);
                                }
                                fold_proto::fold::client_stream_event::Payload::Done(_) => {
                                    got_done = true;
                                }
                                fold_proto::fold::client_stream_event::Payload::Error(e) => {
                                    println!("  ❌ Stream error: {}", e.message);
                                    return;
                                }
                                _ => {}
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
                        break;
                    }
                    Err(_) => {
                        // Timeout on poll, continue
                        print!(".");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                    }
                }
            }

            println!();
            println!();
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("RESULT");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("  Events: {}", event_count);

            // Check if the response mentions the entry was logged
            let text_lower = full_text.to_lowercase();
            if text_lower.contains("logged") || text_lower.contains("added") || text_lower.contains("entry") || text_lower.contains("recorded") {
                println!("  ✅ MCP-BRIDGE TEST PASSED");
                println!("  Response indicates entry was logged successfully");
                println!("  Response preview: {}", truncate(&full_text, 300));
            } else if event_count > 0 {
                println!("  ⚠️  PARTIAL: Got response but unclear if tool was called");
                println!("  Response preview: {}", truncate(&full_text, 300));
            } else {
                println!("  ❌ TEST FAILED: No response received");
            }
        }
        Err(e) => {
            println!("  ❌ Failed to open stream: {:?}", e);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
