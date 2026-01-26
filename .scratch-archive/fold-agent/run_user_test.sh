#!/bin/bash
# ABOUTME: Run user simulation test
# ABOUTME: Requires claude CLI to be installed and configured

set -e

cd "$(dirname "$0")/.."

echo "=== User Simulation Test ==="
echo ""

# Create integration test file
mkdir -p crates/fold-core/tests
TEST_FILE="crates/fold-core/tests/user_simulation.rs"

cat > "$TEST_FILE" << 'TESTEOF'
use fold_core::backend::{DirectCliBackend, DirectCliConfig};
use fold_core::{Config, Fold, IncomingMessage, OutgoingEvent};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
#[ignore]
async fn user_sends_message() {
    println!("\n=== User Simulation Test ===\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.db");

    let mut config = Config::default();
    config.database.path = Some(db_path);
    config.claude.timeout_secs = 30;

    let cli_config = DirectCliConfig {
        binary: "claude".to_string(),
        working_dir: PathBuf::from("."),
        timeout_secs: config.claude.timeout_secs,
    };
    let backend = Arc::new(DirectCliBackend::new(cli_config));
    let fold = Fold::new(&config, backend).await.expect("Failed to create Fold");

    let incoming = IncomingMessage {
        thread_id: "test-thread".to_string(),
        sender: "user".to_string(),
        content: "What is 2+2? Just say the number.".to_string(),
        frontend: "test".to_string(),
        attachments: vec![],
    };

    println!("Sending: {}", incoming.content);

    let mut stream = fold.handle(incoming).await.expect("Handle failed");

    let mut got_response = false;
    while let Some(event) = stream.next().await {
        match event {
            OutgoingEvent::Thinking => println!("[Thinking]"),
            OutgoingEvent::Text(t) => {
                print!("{}", t);
                got_response = true;
            }
            OutgoingEvent::Done { full_response } => {
                println!("\n[Done: {} chars]", full_response.len());
                break;
            }
            OutgoingEvent::Error(e) => {
                println!("\n[Error: {}]", e);
                break;
            }
            _ => {}
        }
    }

    assert!(got_response, "Should get a response");
    println!("\n=== PASSED ===");
}
TESTEOF

echo "Running user simulation (requires claude CLI)..."
echo ""

# Run the test (ignored by default, so use --ignored)
if cargo test --package fold-core --test user_simulation user_sends_message -- --ignored --nocapture 2>&1; then
    echo ""
    echo "✓ User simulation test PASSED"
else
    echo ""
    echo "✗ User simulation test FAILED"
    exit 1
fi

# Cleanup
rm -f "$TEST_FILE"
