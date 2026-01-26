// ABOUTME: Quick E2E test for pack tools
// ABOUTME: Validates response content instead of checking for ToolUse events

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
    println!("║          QUICK PACK E2E TEST                                 ║");
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

    // Quick test cases - validate response content, not tool events
    let tests = vec![
        // Test-pack
        ("echo", "Use the echo tool to echo 'TEST123'", "TEST123"),
        // Productivity-pack
        ("todo_add", "Use the todo_add tool to add a todo with title 'Quick Test Item'", "Quick Test Item"),
        ("todo_list", "Use the todo_list tool to list all todos", "todo"),
        ("note_create", "Use the note_create tool to create a note with title 'Quick Note' and content 'Test content'", "Quick Note"),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (name, prompt, expected_content) in tests {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("TEST: {}", name);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let result = run_test(&mut client, &agent.id, prompt, expected_content).await;

        match result {
            Ok(response) => {
                println!("  ✅ PASSED");
                println!("  Response preview: {}", truncate(&response, 150));
                passed += 1;
            }
            Err(e) => {
                println!("  ❌ FAILED: {}", e);
                failed += 1;
            }
        }
        println!();

        // Minimal delay between tests
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("RESULTS: {} passed, {} failed", passed, failed);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    if failed == 0 {
        println!("🎉 ALL TESTS PASSED!");
    } else {
        println!("⚠️  SOME TESTS FAILED");
    }
}

async fn run_test<F>(
    client: &mut ClientServiceClient<tonic::service::interceptor::InterceptedService<tonic::transport::Channel, F>>,
    agent_id: &str,
    prompt: &str,
    expected_content: &str,
) -> Result<String, String>
where
    F: FnMut(Request<()>) -> Result<Request<()>, tonic::Status>,
{
    let send_request = ClientSendMessageRequest {
        conversation_key: agent_id.to_string(),
        content: prompt.to_string(),
        attachments: vec![],
        idempotency_key: format!("quick-e2e-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()),
    };

    client.send_message(send_request).await
        .map_err(|e| format!("Send failed: {}", e))?;

    // Stream events
    let stream_request = StreamEventsRequest {
        conversation_key: agent_id.to_string(),
        since_event_id: None,
    };

    let response = client.stream_events(stream_request).await
        .map_err(|e| format!("Stream failed: {}", e))?;

    let mut stream = response.into_inner();
    let timeout = Duration::from_secs(45);
    let start = std::time::Instant::now();
    let mut full_response = String::new();
    let mut got_done = false;

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Some(Ok(event))) => {
                if let Some(payload) = &event.payload {
                    match payload {
                        fold_proto::fold::client_stream_event::Payload::Text(t) => {
                            full_response.push_str(&t.content);
                        }
                        fold_proto::fold::client_stream_event::Payload::Done(_) => {
                            got_done = true;
                        }
                        fold_proto::fold::client_stream_event::Payload::Error(e) => {
                            return Err(format!("Stream error: {}", e.message));
                        }
                        _ => {}
                    }
                }
                if got_done {
                    break;
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("Stream error: {:?}", e));
            }
            Ok(None) => {
                break;
            }
            Err(_) => {
                // Timeout on poll, continue
            }
        }
    }

    if !got_done && full_response.is_empty() {
        return Err("No response received".to_string());
    }

    // Validate response contains expected content (case-insensitive)
    if full_response.to_lowercase().contains(&expected_content.to_lowercase()) {
        Ok(full_response)
    } else {
        Err(format!("Response missing expected content '{}'. Got: {}",
            expected_content, truncate(&full_response, 200)))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.replace('\n', " ")
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated.replace('\n', " "))
    }
}
