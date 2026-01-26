// ABOUTME: Scenario test for message sending and streaming
// ABOUTME: Tests end-to-end message flow with real gateway and agent

use fold_client::{FoldClient, StateCallback, StreamCallback, StreamEvent, ConnectionStatus};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};
use std::time::{Duration, Instant};

struct TestState {
    received_events: AtomicU32,
    received_text: AtomicBool,
    received_done: AtomicBool,
    connected: AtomicBool,
    last_text: Mutex<String>,
}

impl Default for TestState {
    fn default() -> Self {
        Self {
            received_events: AtomicU32::new(0),
            received_text: AtomicBool::new(false),
            received_done: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            last_text: Mutex::new(String::new()),
        }
    }
}

// Wrapper to share state between callbacks
struct StateCallbackWrapper(Arc<TestState>);
struct StreamCallbackWrapper(Arc<TestState>);

impl StateCallback for StateCallbackWrapper {
    fn on_connection_status(&self, status: ConnectionStatus) {
        println!("  [STATE] Connection status: {:?}", status);
        if status == ConnectionStatus::Connected {
            self.0.connected.store(true, Ordering::SeqCst);
        }
    }
    fn on_messages_changed(&self, _agent_id: String) {}
    fn on_queue_changed(&self, _agent_id: String, _count: u32) {}
    fn on_unread_changed(&self, _agent_id: String, _count: u32) {}
    fn on_streaming_changed(&self, agent_id: String, streaming: bool) {
        println!("  [STATE] Agent {} streaming: {}", agent_id, streaming);
    }
}

impl StreamCallback for StreamCallbackWrapper {
    fn on_event(&self, agent_id: String, event: StreamEvent) {
        self.0.received_events.fetch_add(1, Ordering::SeqCst);
        match &event {
            StreamEvent::Text { content } => {
                self.0.received_text.store(true, Ordering::SeqCst);
                let preview = if content.len() > 80 { &content[..80] } else { content };
                println!("  [STREAM] {} - Text: {}", agent_id, preview);
                *self.0.last_text.lock().unwrap() = content.clone();
            }
            StreamEvent::Done => {
                self.0.received_done.store(true, Ordering::SeqCst);
                println!("  [STREAM] {} - Done", agent_id);
            }
            StreamEvent::ToolUse { name, .. } => {
                println!("  [STREAM] {} - Tool use: {}", agent_id, name);
            }
            StreamEvent::ToolResult { .. } => {
                println!("  [STREAM] {} - Tool result", agent_id);
            }
            StreamEvent::Usage { info } => {
                println!("  [STREAM] {} - Usage: input={}, output={}",
                    agent_id, info.input_tokens, info.output_tokens);
            }
            StreamEvent::Error { message } => {
                println!("  [STREAM] {} - ERROR: {}", agent_id, message);
            }
            StreamEvent::Thinking { content } => {
                let preview = if content.len() > 50 { &content[..50] } else { content };
                println!("  [STREAM] {} - Thinking: {}...", agent_id, preview);
            }
            StreamEvent::ToolState { state, detail } => {
                println!("  [STREAM] {} - Tool state: {} ({})", agent_id, state, detail);
            }
        }
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           MESSAGE STREAMING TEST                             ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let gateway_url = "http://fold-gateway.porpoise-alkaline.ts.net:50051";
    let ssh_key_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/fold/agent_key");

    if !ssh_key_path.exists() {
        println!("❌ SSH key not found at {:?}", ssh_key_path);
        std::process::exit(1);
    }

    // Create shared state
    let state = Arc::new(TestState::default());

    println!("Creating authenticated client...");
    let client = match FoldClient::new_with_auth(gateway_url.to_string(), &ssh_key_path) {
        Ok(c) => c,
        Err(e) => {
            println!("❌ Failed to create client: {:?}", e);
            std::process::exit(1);
        }
    };

    // Set up callbacks with shared state
    client.set_state_callback(Box::new(StateCallbackWrapper(Arc::clone(&state))));
    client.set_stream_callback(Box::new(StreamCallbackWrapper(Arc::clone(&state))));

    // Test health
    println!("Testing connection...");
    if let Err(e) = client.check_health() {
        println!("❌ Health check failed: {:?}", e);
        std::process::exit(1);
    }
    println!("✅ Connected to gateway");
    println!();

    // Get agents
    println!("Fetching agents...");
    let agents = match client.refresh_agents() {
        Ok(a) => a,
        Err(e) => {
            println!("❌ Failed to get agents: {:?}", e);
            std::process::exit(1);
        }
    };

    let connected_agents: Vec<_> = agents.iter().filter(|a| a.connected).collect();
    println!("Found {} connected agent(s)", connected_agents.len());

    if connected_agents.is_empty() {
        println!("⚠️  No connected agents - skipping message test");
        std::process::exit(0);
    }

    // Pick the first connected agent (prefer aster-fold-agent if available)
    let target_agent = connected_agents.iter()
        .find(|a| a.name.contains("aster"))
        .or_else(|| connected_agents.first())
        .unwrap();

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("SENDING TEST MESSAGE");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Target: {} ({})", target_agent.name, target_agent.id);
    println!("  Backend: {}", target_agent.backend);
    println!();

    // Send a simple test message
    let test_message = "Respond with exactly: PONG";
    println!("  Message: {}", test_message);
    println!();

    let start = Instant::now();
    if let Err(e) = client.send_message(target_agent.id.clone(), test_message.to_string()) {
        println!("❌ Failed to send message: {:?}", e);
        std::process::exit(1);
    }
    println!("  Message sent, waiting for response...");
    println!();

    // Wait for streaming events (with timeout)
    let timeout = Duration::from_secs(120); // 2 minute timeout for LLM response
    while start.elapsed() < timeout {
        if state.received_done.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let elapsed = start.elapsed();
    let event_count = state.received_events.load(Ordering::SeqCst);
    let last_text = state.last_text.lock().unwrap().clone();

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("RESULTS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Time elapsed: {:.2}s", elapsed.as_secs_f64());
    println!("  Events received: {}", event_count);
    println!("  Received text: {}", state.received_text.load(Ordering::SeqCst));
    println!("  Received done: {}", state.received_done.load(Ordering::SeqCst));
    if !last_text.is_empty() {
        println!("  Response preview: {}", if last_text.len() > 100 { &last_text[..100] } else { &last_text });
    }
    println!();

    if state.received_done.load(Ordering::SeqCst) && state.received_text.load(Ordering::SeqCst) {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  ✅ MESSAGE TEST PASSED                                      ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
    } else if elapsed >= timeout {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  ⚠️  MESSAGE TEST TIMED OUT (2 min)                           ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        std::process::exit(1);
    } else {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  ❌ MESSAGE TEST FAILED                                      ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        std::process::exit(1);
    }
}
