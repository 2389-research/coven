// ABOUTME: E2E test for all pack tools
// ABOUTME: Tests echo (test-pack) and productivity tools (productivity-pack)

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
    println!("║          ALL PACKS E2E TEST                                  ║");
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

    // Test cases
    let tests = vec![
        // Test-pack
        ("TEST-PACK: echo", "Use the echo tool to echo 'Hello from E2E test!'"),

        // Productivity-pack: Todos
        ("PRODUCTIVITY: todo_add", "Use the todo_add tool to add a todo with title 'Buy groceries' and due_date '2025-02-01'"),
        ("PRODUCTIVITY: todo_list", "Use the todo_list tool to list all todos"),
        ("PRODUCTIVITY: todo_complete", "Use the todo_complete tool to complete the todo you just created (use the ID from the previous response)"),

        // Productivity-pack: Notes
        ("PRODUCTIVITY: note_create", "Use the note_create tool to create a note with title 'Meeting Notes' and content 'Discussed project timeline and milestones' and tags ['work', 'meetings']"),
        ("PRODUCTIVITY: note_search", "Use the note_search tool to search for notes with query 'meeting'"),
        ("PRODUCTIVITY: note_read", "Use the note_read tool to read the note you just created (use the ID from the search results)"),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (name, prompt) in tests {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("TEST: {}", name);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let result = run_test(&mut client, &agent.id, prompt).await;

        match result {
            Ok(response) => {
                println!("  ✅ PASSED");
                println!("  Response preview: {}", truncate(&response, 200));
                passed += 1;
            }
            Err(e) => {
                println!("  ❌ FAILED: {}", e);
                failed += 1;
            }
        }
        println!();

        // Small delay between tests
        tokio::time::sleep(Duration::from_millis(500)).await;
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
) -> Result<String, String>
where
    F: FnMut(Request<()>) -> Result<Request<()>, tonic::Status>,
{
    let send_request = ClientSendMessageRequest {
        conversation_key: agent_id.to_string(),
        content: prompt.to_string(),
        attachments: vec![],
        idempotency_key: format!("e2e-{}", std::time::SystemTime::now()
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
    let timeout = Duration::from_secs(60);
    let start = std::time::Instant::now();
    let mut full_response = String::new();
    let mut got_done = false;
    let mut tool_called = false;

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
                        fold_proto::fold::client_stream_event::Payload::ToolUse(tu) => {
                            tool_called = true;
                            println!("  Tool: {} (id={})", tu.name, tu.id);
                        }
                        fold_proto::fold::client_stream_event::Payload::ToolResult(tr) => {
                            let status = if tr.is_error { "error" } else { "success" };
                            println!("  Result: {} - {}", status, truncate(&tr.output, 100));
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

    if !tool_called {
        return Err("Tool was not called".to_string());
    }

    if !got_done && full_response.is_empty() {
        return Err("No response received".to_string());
    }

    Ok(full_response)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.replace('\n', " ")
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated.replace('\n', " "))
    }
}
