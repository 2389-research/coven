// ABOUTME: Real user simulation test for fold
// ABOUTME: Tests the full stack from Fold router through DirectCli backend

use fold_core::backend::{DirectCliBackend, DirectCliConfig};
use fold_core::{Config, Fold, IncomingMessage, OutgoingEvent};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// Simulates a real user sending a message through fold
/// This exercises the full stack:
/// 1. Fold router receives IncomingMessage
/// 2. Router creates/retrieves session
/// 3. DirectCli backend spawns claude CLI
/// 4. Events stream back through router to "frontend"
#[tokio::test]
#[ignore] // Requires claude CLI and takes time
async fn user_sends_first_message_new_thread() {
    println!("\n=== User Simulation: First Message to New Thread ===\n");

    // Create temp directory for test database
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");

    // Create config with our test database
    let mut config = Config::default();
    config.database.path = Some(db_path);
    config.claude.timeout_secs = 60;

    // Create DirectCli backend (using real claude CLI)
    let cli_config = DirectCliConfig {
        binary: "claude".to_string(),
        working_dir: PathBuf::from("."),
        timeout_secs: config.claude.timeout_secs,
    };
    let backend = Arc::new(DirectCliBackend::new(cli_config));

    // Create Fold router
    let fold = Fold::new(&config, backend)
        .await
        .expect("Failed to create Fold router");

    // Simulate user sending a message
    let incoming = IncomingMessage {
        thread_id: "user-test-thread-001".to_string(),
        sender: "test-user".to_string(),
        content: "What is 2 + 2? Reply with just the number.".to_string(),
        frontend: "test".to_string(),
        attachments: vec![],
    };

    println!("Sending message: {}", incoming.content);
    println!("Thread ID: {}", incoming.thread_id);
    println!("");

    let result = fold.handle(incoming).await;

    match result {
        Ok(mut stream) => {
            let mut events_received = 0;
            let mut got_thinking = false;
            let mut got_text = false;
            let mut got_done = false;
            let mut full_response = String::new();

            while let Some(event) = stream.next().await {
                events_received += 1;
                match &event {
                    OutgoingEvent::Thinking => {
                        println!("[{}] Thinking...", events_received);
                        got_thinking = true;
                    }
                    OutgoingEvent::Text(t) => {
                        print!("{}", t);
                        got_text = true;
                    }
                    OutgoingEvent::Done { full_response: fr } => {
                        println!("\n[{}] Done ({} chars)", events_received, fr.len());
                        got_done = true;
                        full_response = fr.clone();
                        break;
                    }
                    OutgoingEvent::Error(e) => {
                        println!("\n[{}] Error: {}", events_received, e);
                        // Don't panic - might be expected (e.g., session expired)
                        break;
                    }
                    OutgoingEvent::ToolUse { name, .. } => {
                        println!("[{}] Tool: {}", events_received, name);
                    }
                    OutgoingEvent::ToolResult { .. } => {
                        println!("[{}] Tool result", events_received);
                    }
                    OutgoingEvent::File { filename, .. } => {
                        println!("[{}] File: {}", events_received, filename);
                    }
                }
            }

            println!("\n--- Results ---");
            println!("Events received: {}", events_received);
            println!("Got thinking: {}", got_thinking);
            println!("Got text: {}", got_text);
            println!("Got done: {}", got_done);
            println!("Response: {}", full_response.chars().take(200).collect::<String>());

            assert!(got_thinking, "Should receive Thinking event");
            assert!(got_text || got_done, "Should receive response");
        }
        Err(e) => {
            panic!("Failed to handle message: {}", e);
        }
    }

    println!("\n=== Test Passed ===");
}

/// Simulates user sending second message to existing thread
/// This tests session resume functionality
#[tokio::test]
#[ignore]
async fn user_sends_second_message_existing_thread() {
    println!("\n=== User Simulation: Second Message to Existing Thread ===\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");

    let mut config = Config::default();
    config.database.path = Some(db_path);
    config.claude.timeout_secs = 60;

    let cli_config = DirectCliConfig {
        binary: "claude".to_string(),
        working_dir: PathBuf::from("."),
        timeout_secs: config.claude.timeout_secs,
    };
    let backend = Arc::new(DirectCliBackend::new(cli_config));
    let fold = Fold::new(&config, backend).await.expect("Failed to create Fold");

    let thread_id = "user-test-thread-002";

    // First message
    println!("=== First Message ===");
    let msg1 = IncomingMessage {
        thread_id: thread_id.to_string(),
        sender: "user".to_string(),
        content: "Remember the number 42.".to_string(),
        frontend: "test".to_string(),
        attachments: vec![],
    };

    let mut stream1 = fold.handle(msg1).await.expect("First message failed");
    while let Some(event) = stream1.next().await {
        match event {
            OutgoingEvent::Text(t) => print!("{}", t),
            OutgoingEvent::Done { .. } => {
                println!("\n[Done]");
                break;
            }
            OutgoingEvent::Error(e) => {
                println!("\n[Error: {}]", e);
                break;
            }
            _ => {}
        }
    }

    // Second message - should resume session
    println!("\n=== Second Message (should resume) ===");
    let msg2 = IncomingMessage {
        thread_id: thread_id.to_string(),
        sender: "user".to_string(),
        content: "What number did I ask you to remember?".to_string(),
        frontend: "test".to_string(),
        attachments: vec![],
    };

    let mut stream2 = fold.handle(msg2).await.expect("Second message failed");
    let mut response = String::new();
    while let Some(event) = stream2.next().await {
        match event {
            OutgoingEvent::Text(t) => {
                print!("{}", t);
                response.push_str(&t);
            }
            OutgoingEvent::Done { full_response } => {
                println!("\n[Done]");
                response = full_response;
                break;
            }
            OutgoingEvent::Error(e) => {
                println!("\n[Error: {}]", e);
                break;
            }
            _ => {}
        }
    }

    // Check if Claude remembered
    let remembered = response.contains("42");
    println!("\nClaude remembered '42': {}", remembered);
    assert!(remembered, "Claude should remember the number from previous message");

    println!("\n=== Test Passed ===");
}
