// ABOUTME: E2E test for DirectCli backend with pack tools
// ABOUTME: Tests pack tools accessed via MCP endpoint through claude CLI

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
    println!("║        CLI AGENT PACK TOOLS E2E TEST                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let gateway_url = "http://localhost:50051";
    let ssh_key_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/fold/agent_key");

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

    // Find the CLI agent specifically
    let agents = match client.list_agents(ListAgentsRequest { workspace: None }).await {
        Ok(response) => response.into_inner().agents,
        Err(e) => {
            println!("❌ Failed to list agents: {:?}", e);
            return;
        }
    };

    println!("Available agents:");
    for a in &agents {
        let status = if a.connected { "🟢" } else { "🔴" };
        println!("  {} {} (backend: {})", status, a.name, a.backend);
    }
    println!();

    let agent = match agents.iter().find(|a| a.connected && a.backend == "cli") {
        Some(a) => a,
        None => {
            println!("❌ No connected CLI agent found");
            return;
        }
    };

    println!("✅ Using CLI agent: {} ({})", agent.name, agent.id);
    println!();

    // Test: ask to use the echo pack tool
    let test_prompt = "Use the echo tool to echo 'CLI agent pack test successful!'";

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST: echo tool via CLI agent + MCP");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Prompt: {}", test_prompt);
    println!();

    let send_request = ClientSendMessageRequest {
        conversation_key: agent.id.clone(),
        content: test_prompt.to_string(),
        attachments: vec![],
        idempotency_key: format!("cli-e2e-{}", std::time::SystemTime::now()
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
            let timeout = Duration::from_secs(60);
            let start = std::time::Instant::now();
            let mut event_count = 0;
            let mut got_done = false;
            let mut full_text = String::new();

            while start.elapsed() < timeout {
                match tokio::time::timeout(Duration::from_secs(3), stream.next()).await {
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

            let text_lower = full_text.to_lowercase();
            if text_lower.contains("cli agent pack test successful") || text_lower.contains("echoed") {
                println!("  ✅ CLI AGENT PACK TEST PASSED");
                println!("  Response contains expected echo content");
            } else if event_count > 0 {
                println!("  ⚠️  Got response but echo content not confirmed");
            } else {
                println!("  ❌ TEST FAILED: No response received");
            }

            if !full_text.is_empty() {
                println!();
                println!("  Response preview:");
                let preview: String = full_text.chars().take(500).collect();
                for line in preview.replace('\n', "\n  ").lines().take(10) {
                    println!("    {}", line);
                }
            }
        }
        Err(e) => {
            println!("  ❌ Failed to open stream: {:?}", e);
        }
    }
}
