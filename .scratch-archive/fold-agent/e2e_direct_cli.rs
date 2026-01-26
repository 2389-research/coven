// ABOUTME: End-to-end scenario test for DirectCli backend
// ABOUTME: Actually spawns Claude CLI and verifies event flow

use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

// This test requires the claude CLI to be installed and configured
// Run with: cargo test --package fold-core e2e_direct_cli -- --ignored --nocapture

#[tokio::test]
#[ignore] // Only run when explicitly requested
async fn e2e_new_session_receives_session_init() {
    use fold_core::backend::{Backend, BackendEvent, DirectCliBackend, DirectCliConfig};
    use futures::StreamExt;

    let config = DirectCliConfig {
        binary: "claude".to_string(),
        working_dir: PathBuf::from("."),
        timeout_secs: 30,
    };

    let backend = DirectCliBackend::new(config);

    // Send a simple message as a NEW session
    let result = backend
        .send("test-session-e2e", "Say hello in exactly 3 words", true)
        .await;

    match result {
        Ok(mut stream) => {
            let mut saw_thinking = false;
            let mut saw_session_init = false;
            let mut saw_text = false;
            let mut saw_done = false;
            let mut session_id = String::new();

            // Process events with timeout
            let process_result = timeout(Duration::from_secs(60), async {
                while let Some(event) = stream.next().await {
                    match &event {
                        BackendEvent::Thinking => {
                            println!("✓ Received Thinking event");
                            saw_thinking = true;
                        }
                        BackendEvent::SessionInit { session_id: sid } => {
                            println!("✓ Received SessionInit: {}", sid);
                            saw_session_init = true;
                            session_id = sid.clone();
                        }
                        BackendEvent::Text(t) => {
                            println!("✓ Received Text: {}...", &t[..t.len().min(50)]);
                            saw_text = true;
                        }
                        BackendEvent::Done { full_response } => {
                            println!("✓ Received Done ({} chars)", full_response.len());
                            saw_done = true;
                            break;
                        }
                        BackendEvent::Error(e) => {
                            println!("✗ Received Error: {}", e);
                            panic!("Unexpected error: {}", e);
                        }
                        other => {
                            println!("  Received {:?}", other);
                        }
                    }
                }
            })
            .await;

            assert!(process_result.is_ok(), "Test timed out");
            assert!(saw_thinking, "Should receive Thinking event");
            assert!(saw_session_init, "Should receive SessionInit event for new session");
            assert!(!session_id.is_empty(), "Session ID should not be empty");
            assert!(saw_text || saw_done, "Should receive some response");

            println!("\n=== E2E Test Passed ===");
            println!("Session ID from Claude: {}", session_id);
        }
        Err(e) => {
            panic!("Failed to send message: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn e2e_timeout_kills_process() {
    use fold_core::backend::{Backend, BackendEvent, DirectCliBackend, DirectCliConfig};
    use futures::StreamExt;

    let config = DirectCliConfig {
        binary: "claude".to_string(),
        working_dir: PathBuf::from("."),
        timeout_secs: 2, // Very short timeout
    };

    let backend = DirectCliBackend::new(config);

    // Send a message that will take longer than 2 seconds
    let result = backend
        .send("test-timeout", "Write a 500 word essay about Rust programming", true)
        .await;

    match result {
        Ok(mut stream) => {
            let mut saw_timeout_error = false;

            while let Some(event) = stream.next().await {
                if let BackendEvent::Error(e) = event {
                    if e.contains("timed out") {
                        println!("✓ Received timeout error: {}", e);
                        saw_timeout_error = true;
                        break;
                    }
                }
            }

            assert!(saw_timeout_error, "Should receive timeout error");
            println!("\n=== Timeout Test Passed ===");
        }
        Err(e) => {
            panic!("Failed to send message: {}", e);
        }
    }
}
