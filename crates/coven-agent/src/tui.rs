// ABOUTME: TUI runner for coven-agent - visual display of agent activity
// ABOUTME: Non-interactive, shows colored logs of events and status

use anyhow::{Result, bail};

/// Generate pack tools status message based on backend type.
/// Returns None if tool_count is 0.
fn pack_tools_message(tool_count: usize, is_cli_backend: bool) -> Option<String> {
    if tool_count == 0 {
        return None;
    }
    if is_cli_backend {
        Some(format!("Pack tools: {} available", tool_count))
    } else {
        Some(format!(
            "Pack tools: {} detected (mux TUI support pending)",
            tool_count
        ))
    }
}
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use coven_core::backend::{
    ApprovalCallback, Backend, DirectCliBackend, DirectCliConfig, MuxBackend, MuxConfig,
};
use coven_core::{Config, Coven, IncomingMessage, OutgoingEvent};
use coven_proto::coven_control_client::CovenControlClient;
use coven_proto::{AgentMessage, MessageResponse, RegisterAgent, agent_message, server_message};
use coven_ssh::{
    SshAuthCredentials, compute_fingerprint, default_agent_key_path, load_or_generate_key,
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tonic::transport::Channel;

use crate::metadata::AgentMetadata;

use coven_connect::event::convert_event_to_response;
use coven_connect::MAX_REGISTRATION_ATTEMPTS;

/// Maximum number of log lines to keep
const MAX_LOG_LINES: usize = 1000;

/// Exit goodbyes - randomly selected on quit
const EXIT_GOODBYES: &[&str] = &[
    "See you soon!",
    "Until next time!",
    "Goodbye for now!",
    "Catch you later!",
    "Stay orchestrated!",
    "Keep automating!",
    "The agents await your return.",
    "Till we meet again!",
    "Happy building!",
    "Go make something great!",
];

/// Exit taglines - randomly selected on quit
const EXIT_TAGLINES: &[&str] = &[
    "The future of work is orchestrated.",
    "Agents at your service.",
    "Go forth and automate.",
    "The hive mind rests.",
    "Async dreams await.",
    "May your pipelines flow.",
    "Orchestration complete.",
    "The swarm sleeps.",
];

/// Terminal guard for safe cleanup - handles both raw mode and alternate screen
struct TerminalGuard {
    entered_alt_screen: bool,
}

impl TerminalGuard {
    fn new() -> Self {
        Self {
            entered_alt_screen: false,
        }
    }

    fn set_alt_screen(&mut self) {
        self.entered_alt_screen = true;
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        if self.entered_alt_screen {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }
}

/// Print a branded farewell message after the TUI exits
fn print_exit_message() {
    use rand::seq::SliceRandom;

    let mut rng = rand::thread_rng();
    let goodbye = EXIT_GOODBYES.choose(&mut rng).unwrap_or(&"Goodbye!");
    let tagline = EXIT_TAGLINES.choose(&mut rng).unwrap_or(&"");

    // ANSI color codes for styling
    const CYAN: &str = "\x1b[36m";
    const CYAN_BOLD: &str = "\x1b[1;36m";
    const DIM: &str = "\x1b[2m";
    const RESET: &str = "\x1b[0m";

    println!();
    println!("  {CYAN}{goodbye}{RESET}");
    println!();
    println!(
        "  {DIM}─═══─{RESET} {CYAN}⬡{RESET}  {CYAN_BOLD}2389.ai{RESET} {CYAN}⬡{RESET}  {DIM}─═══─{RESET}"
    );
    println!("  {DIM}{tagline}{RESET}");
    println!();
}

/// A content block that can span multiple lines
#[derive(Clone)]
struct ContentBlock {
    timestamp: String,
    kind: BlockKind,
    content: String,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    System,      // Info messages, connection status
    UserMessage, // Incoming user message
    Assistant,   // Assistant text response
    Thinking,    // Thinking indicator
    ToolCall,    // Tool being called
    ToolOutput,  // Tool result
    Error,       // Error message
}

impl BlockKind {
    fn header_color(&self) -> Color {
        match self {
            BlockKind::System => Color::Blue,
            BlockKind::UserMessage => Color::Magenta,
            BlockKind::Assistant => Color::Cyan,
            BlockKind::Thinking => Color::Yellow,
            BlockKind::ToolCall => Color::Yellow,
            BlockKind::ToolOutput => Color::Green,
            BlockKind::Error => Color::Red,
        }
    }

    fn content_color(&self) -> Color {
        match self {
            BlockKind::System => Color::DarkGray,
            BlockKind::UserMessage => Color::White,
            BlockKind::Assistant => Color::White,
            BlockKind::Thinking => Color::DarkGray,
            BlockKind::ToolCall => Color::DarkGray,
            BlockKind::ToolOutput => Color::DarkGray,
            BlockKind::Error => Color::Red,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            BlockKind::System => "SYSTEM",
            BlockKind::UserMessage => "USER",
            BlockKind::Assistant => "ASSISTANT",
            BlockKind::Thinking => "THINKING",
            BlockKind::ToolCall => "TOOL",
            BlockKind::ToolOutput => "RESULT",
            BlockKind::Error => "ERROR",
        }
    }

    /// Whether this block type should have visual separation (blank line before)
    fn needs_separator(&self) -> bool {
        matches!(self, BlockKind::UserMessage | BlockKind::Assistant)
    }
}

/// Parse a server URL like "http://127.0.0.1:50051" into (host, port)
fn parse_server_url(url: &str) -> (String, String) {
    // Remove protocol prefix if present
    let without_proto = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    // Split host and port
    if let Some((host, port)) = without_proto.rsplit_once(':') {
        (format!("http://{}", host), port.to_string())
    } else {
        (format!("http://{}", without_proto), "50051".to_string())
    }
}

/// TUI application state
struct App {
    agent_name: String,
    agent_id: String,
    instance_id: String,
    server_host: String,
    server_port: String,
    backend: String,
    working_dir: String,
    status: String,
    last_message: String,
    pack_tools_count: usize,
    blocks: VecDeque<ContentBlock>,
    /// Scroll offset in visual lines (not blocks)
    scroll_offset: usize,
    /// Cached visual lines for rendering
    visual_lines: Vec<VisualLine>,
    /// Whether visual lines need recalculating
    lines_dirty: bool,
    should_quit: bool,
    title_changed: bool,
}

/// A single visual line for rendering
#[derive(Clone)]
struct VisualLine {
    spans: Vec<(String, Color)>,
}

impl App {
    fn new(agent_id: &str, server: &str, backend: &str, working_dir: &str) -> Self {
        // Parse server URL into host and port
        let (host, port) = parse_server_url(server);

        Self {
            agent_name: agent_id.to_string(),
            agent_id: agent_id.to_string(),
            instance_id: String::new(),
            server_host: host,
            server_port: port,
            backend: backend.to_string(),
            working_dir: working_dir.to_string(),
            status: "Initializing...".to_string(),
            last_message: "Ready".to_string(),
            pack_tools_count: 0,
            blocks: VecDeque::with_capacity(MAX_LOG_LINES),
            scroll_offset: 0,
            visual_lines: Vec::new(),
            lines_dirty: true,
            should_quit: false,
            title_changed: true,
        }
    }

    /// Get the terminal title string
    fn terminal_title(&self) -> String {
        let dir_name = std::path::Path::new(&self.working_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.working_dir);

        let msg_preview: String = self.last_message.chars().take(40).collect();
        let ellipsis = if self.last_message.len() > 40 {
            "..."
        } else {
            ""
        };

        format!(
            "{} - {} - {}{}",
            self.agent_name, dir_name, msg_preview, ellipsis
        )
    }

    fn set_last_message(&mut self, msg: impl Into<String>) {
        self.last_message = msg.into();
        self.title_changed = true;
    }

    fn add_block(&mut self, kind: BlockKind, content: impl Into<String>) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.blocks.push_back(ContentBlock {
            timestamp,
            kind,
            content: content.into(),
        });

        if self.blocks.len() > MAX_LOG_LINES {
            self.blocks.pop_front();
        }

        self.lines_dirty = true;
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    /// Rebuild visual lines from content blocks for a given width
    fn rebuild_visual_lines(&mut self, width: usize) {
        self.visual_lines.clear();

        let content_width = width.saturating_sub(4); // Leave margin for indent
        if content_width == 0 {
            return;
        }

        let mut prev_kind: Option<BlockKind> = None;

        for block in &self.blocks {
            // Add separator line before major blocks
            if block.kind.needs_separator() && prev_kind.is_some() {
                self.visual_lines.push(VisualLine { spans: vec![] });
            }

            // Header line: [TIME] [LABEL]
            self.visual_lines.push(VisualLine {
                spans: vec![
                    (format!("{} ", block.timestamp), Color::DarkGray),
                    (
                        format!("[{}]", block.kind.label()),
                        block.kind.header_color(),
                    ),
                ],
            });

            // Content lines - wrap text properly
            let content_color = block.kind.content_color();
            for line in block.content.lines() {
                if line.is_empty() {
                    self.visual_lines.push(VisualLine {
                        spans: vec![("    ".to_string(), Color::Reset)],
                    });
                } else {
                    // Word-wrap long lines
                    for wrapped in wrap_text(line, content_width) {
                        self.visual_lines.push(VisualLine {
                            spans: vec![
                                ("    ".to_string(), Color::Reset), // Indent
                                (wrapped, content_color),
                            ],
                        });
                    }
                }
            }

            prev_kind = Some(block.kind);
        }

        self.lines_dirty = false;
    }

    /// Scroll up by n lines
    fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    fn scroll_down(&mut self, n: usize, visible_height: usize) {
        let max_scroll = self.visual_lines.len().saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    /// Scroll to bottom
    fn scroll_to_bottom(&mut self, visible_height: usize) {
        let max_scroll = self.visual_lines.len().saturating_sub(visible_height);
        self.scroll_offset = max_scroll;
    }
}

/// Word-wrap text to fit within width
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();

        if current_width == 0 {
            // First word on line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            // Word fits on current line
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Start new line
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// UI event sent from the agent task to the render task
enum UiEvent {
    Block(BlockKind, String),
    Status(String),
    LastMessage(String),
    InstanceId(String),
    PackToolsCount(usize),
    Quit,
}

pub async fn run(
    server_addr: &str,
    agent_id: &str,
    backend_type: &str,
    working_dir: &std::path::Path,
    capabilities: Vec<String>,
) -> Result<()> {
    // Setup terminal - guard created immediately after raw mode to ensure cleanup on panic
    enable_raw_mode()?;
    let mut _guard = TerminalGuard::new();

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    _guard.set_alt_screen();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        agent_id,
        server_addr,
        backend_type,
        &working_dir.display().to_string(),
    );

    // Channel for UI events from agent task
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(100);

    // Spawn agent task
    let agent_tx = ui_tx.clone();
    let server = server_addr.to_string();
    let id = agent_id.to_string();
    let backend_str = backend_type.to_string();
    let work_dir = working_dir.to_path_buf();
    let caps = capabilities;

    tokio::spawn(async move {
        if let Err(e) = run_agent_task(&agent_tx, &server, &id, &backend_str, &work_dir, caps).await {
            let _ = agent_tx
                .send(UiEvent::Block(
                    BlockKind::Error,
                    format!("Agent error: {}", e),
                ))
                .await;
            // Auto-quit on error so user sees the error and exits
            let _ = agent_tx.send(UiEvent::Quit).await;
        }
        // On graceful disconnect (gateway shutdown), don't auto-quit
        // Let user read the disconnect message and press 'q' to exit
    });

    // Track visible height for scrolling
    let mut visible_height: usize = 10;

    // Main render loop
    loop {
        // Update terminal title if changed
        if app.title_changed {
            execute!(io::stdout(), SetTitle(app.terminal_title()))?;
            app.title_changed = false;
        }

        // Check for UI events (non-blocking)
        while let Ok(event) = ui_rx.try_recv() {
            match event {
                UiEvent::Block(kind, content) => app.add_block(kind, content),
                UiEvent::Status(status) => app.set_status(status),
                UiEvent::LastMessage(msg) => app.set_last_message(msg),
                UiEvent::InstanceId(id) => app.instance_id = id,
                UiEvent::PackToolsCount(count) => app.pack_tools_count = count,
                UiEvent::Quit => app.should_quit = true,
            }
        }

        // Draw UI and get actual visible height
        terminal.draw(|f| {
            visible_height = draw_ui(f, &mut app);
        })?;

        // Always auto-scroll to bottom when new content arrives
        if app.lines_dirty {
            app.scroll_to_bottom(visible_height);
        }

        // Check for keyboard input (non-blocking)
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.scroll_up(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.scroll_down(1, visible_height);
                        }
                        KeyCode::PageUp => {
                            app.scroll_up(10);
                        }
                        KeyCode::PageDown => {
                            app.scroll_down(10, visible_height);
                        }
                        KeyCode::Home => {
                            app.scroll_offset = 0;
                        }
                        KeyCode::End => {
                            app.scroll_to_bottom(visible_height);
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Explicitly drop terminal and guard to restore the terminal before printing
    drop(terminal);
    drop(_guard);

    // Print farewell message
    print_exit_message();

    Ok(())
}

/// Shared state for pending tool approvals - maps tool_id to response sender
type PendingApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

async fn run_agent_task(
    tx: &mpsc::Sender<UiEvent>,
    server_addr: &str,
    agent_id: &str,
    backend_type: &str,
    working_dir: &std::path::Path,
    capabilities: Vec<String>,
) -> Result<()> {
    tx.send(UiEvent::Block(
        BlockKind::System,
        "Loading configuration...".to_string(),
    ))
    .await?;
    let config = Config::load()?;

    // Create shared state for pending approvals
    let pending_approvals: PendingApprovals = Arc::new(Mutex::new(HashMap::new()));

    // Create backend
    // For CLI backend, keep a reference to set MCP endpoint after receiving token
    let mut cli_backend: Option<Arc<DirectCliBackend>> = None;
    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("Initializing {} backend...", backend_type),
    ))
    .await?;
    let backend: Arc<dyn Backend> = match backend_type {
        "mux" => {
            tx.send(UiEvent::Block(
                BlockKind::System,
                "Using MuxBackend (direct Anthropic API)".to_string(),
            ))
            .await?;
            let mux_settings = config.mux.clone();
            let mux_config = MuxConfig {
                model: std::env::var("ANTHROPIC_MODEL").unwrap_or(mux_settings.model),
                max_tokens: std::env::var("ANTHROPIC_MAX_TOKENS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(mux_settings.max_tokens),
                working_dir: working_dir.to_path_buf(),
                global_system_prompt_path: mux_settings
                    .global_system_prompt_path
                    .or_else(|| dirs::home_dir().map(|h| h.join(".mux/system.md"))),
                local_prompt_files: mux_settings.local_prompt_files,
                global_soul_path: mux_settings.global_soul_path,
                agent_soul_path: mux_settings.agent_soul_path,
                soul_files: mux_settings.soul_files,
                mcp_servers: vec![],
                skip_default_tools: false,
                gateway_mcp: None, // Standalone TUI doesn't connect to gateway
            };

            // Create approval callback that waits for gateway response
            // Timeout after 5 minutes to prevent infinite hangs
            const APPROVAL_TIMEOUT_SECS: u64 = 300;

            let approvals = pending_approvals.clone();
            let approval_callback: ApprovalCallback =
                Arc::new(move |tool_id, tool_name, _tool_input| {
                    let approvals = approvals.clone();
                    Box::pin(async move {
                        // Create oneshot channel for this approval
                        let (sender, rx) = oneshot::channel();

                        // Store the sender for when we receive the response
                        {
                            let mut pending = approvals.lock().await;
                            pending.insert(tool_id.clone(), sender);
                        }

                        // Wait for approval response with timeout
                        let timeout = tokio::time::Duration::from_secs(APPROVAL_TIMEOUT_SECS);
                        match tokio::time::timeout(timeout, rx).await {
                            Ok(Ok(approved)) => approved,
                            Ok(Err(_)) => {
                                // Channel closed without response - deny by default
                                false
                            }
                            Err(_) => {
                                // Timeout - clean up and deny
                                eprintln!("Approval timeout for '{}', denying tool", tool_name);
                                let mut pending = approvals.lock().await;
                                pending.remove(&tool_id);
                                false
                            }
                        }
                    })
                        as Pin<Box<dyn std::future::Future<Output = bool> + Send>>
                });

            Arc::new(
                MuxBackend::new(mux_config)
                    .await?
                    .with_approval_callback(approval_callback),
            )
        }
        "cli" => {
            tx.send(UiEvent::Block(
                BlockKind::System,
                format!("Using DirectCliBackend ({})", config.claude.binary),
            ))
            .await?;
            let cli_config = DirectCliConfig {
                binary: config.claude.binary.clone(),
                working_dir: working_dir.to_path_buf(),
                timeout_secs: config.claude.timeout_secs,
                mcp_endpoint: None, // Will be set after receiving Welcome with token
            };
            let backend = Arc::new(DirectCliBackend::new(cli_config));
            cli_backend = Some(backend.clone());
            backend
        }
        _ => bail!("Unknown backend '{}'. Use 'mux' or 'cli'", backend_type),
    };

    let coven = Coven::new(&config, backend).await?;

    // Load or generate SSH key for authentication
    let key_path = default_agent_key_path()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory for SSH key"))?;

    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("Loading SSH key from {}...", key_path.display()),
    ))
    .await?;

    let private_key = load_or_generate_key(&key_path)?;
    let fingerprint = compute_fingerprint(private_key.public_key())?;
    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("SSH fingerprint: {}", fingerprint),
    ))
    .await?;

    // Connect to server
    tx.send(UiEvent::Status("Connecting...".to_string()))
        .await?;
    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("Connecting to gateway at {}...", server_addr),
    ))
    .await?;

    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;
    tx.send(UiEvent::Block(
        BlockKind::System,
        "TCP connection established".to_string(),
    ))
    .await?;

    // Create SSH auth interceptor
    let private_key = Arc::new(private_key);
    let private_key_clone = private_key.clone();
    let ssh_auth_interceptor = move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
        match SshAuthCredentials::new(&private_key_clone) {
            Ok(creds) => {
                if let Err(e) = creds.apply_to_request(&mut req) {
                    return Err(tonic::Status::internal(format!("failed to apply SSH auth: {}", e)));
                }
            }
            Err(e) => {
                return Err(tonic::Status::internal(format!("failed to create SSH auth credentials: {}", e)));
            }
        }
        Ok(req)
    };

    let mut client = CovenControlClient::with_interceptor(channel, ssh_auth_interceptor);

    // Gather metadata for registration
    tx.send(UiEvent::Block(
        BlockKind::System,
        "Gathering environment metadata...".to_string(),
    ))
    .await?;
    let mut metadata = AgentMetadata::gather(working_dir);
    metadata.backend = backend_type.to_string();
    metadata.capabilities = capabilities;
    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("Working directory: {}", metadata.working_directory),
    ))
    .await?;
    tx.send(UiEvent::Block(
        BlockKind::System,
        format!("Backend: {}", metadata.backend),
    ))
    .await?;
    if let Some(ref git) = metadata.git {
        tx.send(UiEvent::Block(
            BlockKind::System,
            format!(
                "Git: {} @ {} {}",
                git.branch,
                git.commit,
                if git.dirty { "(dirty)" } else { "" }
            ),
        ))
        .await?;
    }

    // Registration retry loop - try with incrementing suffix if name is taken
    let mut suffix: usize = 0;
    let (msg_tx, mut inbound) = loop {
        let current_id = if suffix == 0 {
            agent_id.to_string()
        } else {
            format!("{}-{}", agent_id, suffix)
        };

        // Create bidirectional stream
        let (msg_tx, rx) = mpsc::channel::<AgentMessage>(100);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        tx.send(UiEvent::Block(
            BlockKind::System,
            "Opening bidirectional stream...".to_string(),
        ))
        .await?;
        let response = client.agent_stream(outbound).await?;
        let mut inbound = response.into_inner();
        tx.send(UiEvent::Block(
            BlockKind::System,
            "Stream established".to_string(),
        ))
        .await?;

        // Send registration
        tx.send(UiEvent::Block(
            BlockKind::System,
            format!("Sending registration as '{}'...", current_id),
        ))
        .await?;
        msg_tx
            .send(AgentMessage {
                payload: Some(agent_message::Payload::Register(RegisterAgent {
                    agent_id: current_id.clone(),
                    name: current_id.clone(),
                    capabilities: metadata.capabilities.clone(),
                    metadata: Some(metadata.clone().into()),
                    protocol_features: vec!["token_usage".to_string(), "tool_states".to_string()],
                })),
            })
            .await?;

        // Wait for Welcome or RegistrationError
        match inbound.next().await {
            Some(Ok(msg)) => match msg.payload {
                Some(server_message::Payload::Welcome(welcome)) => {
                    tx.send(UiEvent::InstanceId(welcome.instance_id.clone()))
                        .await?;
                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        format!(
                            "Connected to gateway '{}' as '{}'",
                            welcome.server_id, welcome.agent_id
                        ),
                    ))
                    .await?;
                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        format!("Instance ID: {}", welcome.instance_id),
                    ))
                    .await?;

                    // Set MCP endpoint for CLI backend if endpoint and token provided
                    if let Some(ref cli) = cli_backend {
                        if !welcome.mcp_endpoint.is_empty() && !welcome.mcp_token.is_empty() {
                            let mcp_url =
                                crate::build_mcp_url(&welcome.mcp_endpoint, &welcome.mcp_token);
                            tx.send(UiEvent::Block(
                                BlockKind::System,
                                "MCP endpoint: configured (token received)".to_string(),
                            ))
                            .await?;
                            cli.set_mcp_endpoint(mcp_url);
                        }
                    }

                    // Update header with pack tools count (for any backend)
                    let tool_count = welcome.available_tools.len();
                    tx.send(UiEvent::PackToolsCount(tool_count)).await?;

                    // Log message differs by backend type
                    if let Some(msg) = pack_tools_message(tool_count, cli_backend.is_some()) {
                        tx.send(UiEvent::Block(BlockKind::System, msg)).await?;
                    }

                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        format!(
                            "To bind a channel: Slack: /coven bind {} | Matrix: !coven bind {}",
                            welcome.instance_id, welcome.instance_id
                        ),
                    ))
                    .await?;
                    tx.send(UiEvent::Status("Ready".to_string())).await?;
                    if suffix > 0 {
                        tx.send(UiEvent::Block(
                            BlockKind::System,
                            format!(
                                "(Registered as '{}' due to name conflict)",
                                welcome.agent_id
                            ),
                        ))
                        .await?;
                    }
                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        "Waiting for messages...".to_string(),
                    ))
                    .await?;
                    break (msg_tx, inbound);
                }
                Some(server_message::Payload::RegistrationError(err)) => {
                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        format!("Registration rejected: {} (trying with suffix)", err.reason),
                    ))
                    .await?;
                    suffix += 1;
                    if suffix >= MAX_REGISTRATION_ATTEMPTS {
                        bail!(
                            "Failed to register after {} attempts - all IDs taken",
                            MAX_REGISTRATION_ATTEMPTS
                        );
                    }
                    // Stream is dropped here, will create new one in next iteration
                    continue;
                }
                Some(server_message::Payload::Shutdown(shutdown)) => {
                    bail!("Server shutdown during registration: {}", shutdown.reason);
                }
                other => {
                    bail!("Unexpected message during registration: {:?}", other);
                }
            },
            Some(Err(e)) => {
                // Check if this is an AlreadyExists error - if so, retry with suffix
                if e.code() == tonic::Code::AlreadyExists {
                    tx.send(UiEvent::Block(
                        BlockKind::System,
                        format!(
                            "Registration rejected: {} (trying with suffix)",
                            e.message()
                        ),
                    ))
                    .await?;
                    suffix += 1;
                    if suffix >= MAX_REGISTRATION_ATTEMPTS {
                        bail!(
                            "Failed to register after {} attempts - all IDs taken",
                            MAX_REGISTRATION_ATTEMPTS
                        );
                    }
                    // Stream is dropped here, will create new one in next iteration
                    continue;
                }
                return Err(e.into());
            }
            None => {
                bail!("Stream closed before registration completed");
            }
        }
    };

    // Process server messages
    while let Some(msg) = inbound.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tx.send(UiEvent::Block(
                    BlockKind::Error,
                    format!("gRPC stream error: {}", e),
                ))
                .await?;
                return Err(e.into());
            }
        };
        match msg.payload {
            Some(server_message::Payload::Welcome(_)) => {
                // Already handled during registration, ignore duplicate
            }
            Some(server_message::Payload::RegistrationError(_)) => {
                // Already handled during registration, ignore late arrival
            }
            Some(server_message::Payload::SendMessage(send_msg)) => {
                tx.send(UiEvent::Status("Processing...".to_string()))
                    .await?;
                // Update terminal title with message preview
                tx.send(UiEvent::LastMessage(send_msg.content.clone()))
                    .await?;
                // Log the full incoming message
                tx.send(UiEvent::Block(
                    BlockKind::UserMessage,
                    send_msg.content.clone(),
                ))
                .await?;

                let incoming = IncomingMessage {
                    thread_id: send_msg.thread_id.clone(),
                    sender: send_msg.sender.clone(),
                    content: send_msg.content.clone(),
                    frontend: "grpc".to_string(),
                    attachments: vec![],
                };

                let msg_tx_clone = msg_tx.clone();
                let request_id = send_msg.request_id.clone();

                match coven.handle(incoming).await {
                    Ok(mut stream) => {
                        let mut event_count = 0;
                        while let Some(event) = stream.next().await {
                            event_count += 1;

                            // Log the event to UI
                            match &event {
                                OutgoingEvent::Thinking => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::Thinking,
                                        "Thinking...".to_string(),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::Text(t) => {
                                    // Show full text content
                                    tx.send(UiEvent::Block(BlockKind::Assistant, t.clone()))
                                        .await?;
                                }
                                OutgoingEvent::ToolUse { name, input, .. } => {
                                    // Show tool name and input preview
                                    let input_str = input.to_string();
                                    let input_preview: String =
                                        input_str.chars().take(100).collect();
                                    tx.send(UiEvent::Block(
                                        BlockKind::ToolCall,
                                        format!(
                                            "{}: {}{}",
                                            name,
                                            input_preview,
                                            if input_str.len() > 100 { "..." } else { "" }
                                        ),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::ToolResult {
                                    is_error, output, ..
                                } => {
                                    let output_preview: String = output.chars().take(200).collect();
                                    if *is_error {
                                        tx.send(UiEvent::Block(
                                            BlockKind::Error,
                                            format!(
                                                "Error: {}{}",
                                                output_preview,
                                                if output.len() > 200 { "..." } else { "" }
                                            ),
                                        ))
                                        .await?;
                                    } else {
                                        tx.send(UiEvent::Block(
                                            BlockKind::ToolOutput,
                                            format!(
                                                "{}{}",
                                                output_preview,
                                                if output.len() > 200 { "..." } else { "" }
                                            ),
                                        ))
                                        .await?;
                                    }
                                }
                                OutgoingEvent::Done { .. } => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::System,
                                        format!("Complete ({} events)", event_count),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::Error(e) => {
                                    tx.send(UiEvent::Block(BlockKind::Error, e.clone())).await?;
                                }
                                OutgoingEvent::File { filename, .. } => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::System,
                                        format!("File: {}", filename),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::ToolApprovalRequest { name, input, .. } => {
                                    let input_str = input.to_string();
                                    let input_preview: String =
                                        input_str.chars().take(60).collect();
                                    tx.send(UiEvent::Block(
                                        BlockKind::ToolCall,
                                        format!(
                                            "Approve? {}: {}{}",
                                            name,
                                            input_preview,
                                            if input_str.len() > 60 { "..." } else { "" }
                                        ),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::SessionInit { session_id } => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::System,
                                        format!("Session: {}", session_id),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::SessionOrphaned => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::Error,
                                        "Session expired - please retry".to_string(),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::Usage {
                                    input_tokens,
                                    output_tokens,
                                    cache_read_tokens,
                                    thinking_tokens,
                                    ..
                                } => {
                                    tx.send(UiEvent::Block(
                                        BlockKind::System,
                                        format!(
                                            "Usage: in={} out={} cache={} think={}",
                                            input_tokens,
                                            output_tokens,
                                            cache_read_tokens,
                                            thinking_tokens
                                        ),
                                    ))
                                    .await?;
                                }
                                OutgoingEvent::ToolState { id, state, detail } => {
                                    let detail_str = detail
                                        .as_deref()
                                        .map(|d| format!(" ({})", d))
                                        .unwrap_or_default();
                                    tx.send(UiEvent::Block(
                                        BlockKind::System,
                                        format!("Tool {}: {}{}", id, state, detail_str),
                                    ))
                                    .await?;
                                }
                            }

                            let response = convert_event_to_response(&request_id, event).await;
                            if msg_tx_clone.send(response).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tx.send(UiEvent::Block(BlockKind::Error, format!("Error: {}", e)))
                            .await?;
                        let error_response = AgentMessage {
                            payload: Some(agent_message::Payload::Response(MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(coven_proto::message_response::Event::Error(
                                    e.to_string(),
                                )),
                            })),
                        };
                        msg_tx_clone.send(error_response).await?;

                        let done_response = AgentMessage {
                            payload: Some(agent_message::Payload::Response(MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(coven_proto::message_response::Event::Done(
                                    coven_proto::Done {
                                        full_response: format!("Error: {}", e),
                                    },
                                )),
                            })),
                        };
                        msg_tx_clone.send(done_response).await?;
                    }
                }
                tx.send(UiEvent::Status("Ready".to_string())).await?;
            }
            Some(server_message::Payload::Shutdown(shutdown)) => {
                tx.send(UiEvent::Block(
                    BlockKind::System,
                    format!("Server shutdown: {}", shutdown.reason),
                ))
                .await?;
                tx.send(UiEvent::Status("Disconnected".to_string())).await?;
                break;
            }
            Some(server_message::Payload::ToolApproval(approval)) => {
                let status = if approval.approved {
                    "approved"
                } else {
                    "denied"
                };
                tx.send(UiEvent::Block(
                    BlockKind::System,
                    format!("Tool {}: {}", approval.id, status),
                ))
                .await?;

                // Resolve the pending approval
                let mut pending = pending_approvals.lock().await;
                if let Some(sender) = pending.remove(&approval.id) {
                    if sender.send(approval.approved).is_err() {
                        tx.send(UiEvent::Block(
                            BlockKind::Error,
                            format!("Failed to send approval for {}", approval.id),
                        ))
                        .await?;
                    }
                }
            }
            Some(server_message::Payload::InjectContext(inject)) => {
                tx.send(UiEvent::Block(
                    BlockKind::System,
                    format!(
                        "Context injection [{}]: {} chars",
                        inject.injection_id,
                        inject.content.len()
                    ),
                ))
                .await?;
                // TODO: Implement context injection - acknowledge receipt
            }
            Some(server_message::Payload::CancelRequest(cancel)) => {
                tx.send(UiEvent::Block(
                    BlockKind::System,
                    format!("Cancel request: {}", cancel.request_id),
                ))
                .await?;
                // TODO: Implement request cancellation
            }
            Some(server_message::Payload::PackToolResult(result)) => {
                // Pack tool results are handled by the client.rs message loop, not TUI
                tx.send(UiEvent::Block(
                    BlockKind::System,
                    format!("Pack tool result: {}", result.request_id),
                ))
                .await?;
            }
            None => {}
        }
    }

    // Stream ended - gateway disconnected
    tx.send(UiEvent::Block(
        BlockKind::Error,
        "Gateway connection closed".to_string(),
    ))
    .await?;
    tx.send(UiEvent::Status("Disconnected".to_string())).await?;
    tx.send(UiEvent::Block(
        BlockKind::System,
        "The gateway server shut down or the connection was lost. Press 'q' to exit.".to_string(),
    ))
    .await?;

    Ok(())
}

fn draw_ui(f: &mut Frame, app: &mut App) -> usize {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Header
            Constraint::Min(10),   // Logs
            Constraint::Length(1), // Help
        ])
        .split(f.area());

    // Header
    draw_header(f, app, chunks[0]);

    // Logs - returns visible height
    let visible_height = draw_logs(f, app, chunks[1]);

    // Help bar
    let help =
        Paragraph::new(" [q/Esc/Ctrl-C] Quit  [up/down/j/k] Scroll  [PgUp/PgDn] Page  [Home/End] Jump")
            .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[2]);

    visible_height
}

fn draw_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " coven-agent ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Pre-compute truncated strings to avoid lifetime issues
    let agent_id_short: String = app.agent_id.chars().take(16).collect();
    let working_dir_short: String = app.working_dir.chars().take(40).collect();
    let current_time = chrono::Local::now().format("%H:%M:%S").to_string();

    let status_color = if app.status == "Ready" {
        Color::Green
    } else if app.status.contains("Error") {
        Color::Red
    } else {
        Color::Yellow
    };

    // Show bind ID or placeholder if not yet received
    let bind_id_display = if app.instance_id.is_empty() {
        "...".to_string()
    } else {
        app.instance_id.clone()
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
            Span::styled(current_time, Style::default().fg(Color::White)),
            Span::styled("  Agent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.agent_name.clone(), Style::default().fg(Color::White)),
            Span::styled("  ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(agent_id_short, Style::default().fg(Color::White)),
            Span::styled("  Bind: ", Style::default().fg(Color::DarkGray)),
            Span::styled(bind_id_display, Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled("Server: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.server_host.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled("  Port: ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.server_port.clone(), Style::default().fg(Color::Yellow)),
            Span::styled("  Backend: ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.backend.clone(), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.status.clone(), Style::default().fg(status_color)),
            Span::styled("  Packs: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.pack_tools_count.to_string(),
                Style::default().fg(if app.pack_tools_count > 0 {
                    Color::Green
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled("  Dir: ", Style::default().fg(Color::DarkGray)),
            Span::styled(working_dir_short, Style::default().fg(Color::White)),
        ]),
    ];

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

fn draw_logs(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) -> usize {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(
                " Messages ({} blocks, {} lines) ",
                app.blocks.len(),
                app.visual_lines.len()
            ),
            Style::default().fg(Color::DarkGray),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;
    let content_width = inner.width as usize;

    // Rebuild visual lines if dirty or width changed
    if app.lines_dirty || app.visual_lines.is_empty() {
        app.rebuild_visual_lines(content_width);
    }

    // Convert visual lines to ratatui Lines for the visible portion
    let start = app.scroll_offset;
    let end = (start + visible_height).min(app.visual_lines.len());

    let lines: Vec<Line> = app.visual_lines[start..end]
        .iter()
        .map(|vl| {
            let spans: Vec<Span> = vl
                .spans
                .iter()
                .map(|(text, color)| Span::styled(text.clone(), Style::default().fg(*color)))
                .collect();
            Line::from(spans)
        })
        .collect();

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);

    visible_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_tools_message_cli_backend() {
        let msg = pack_tools_message(5, true);
        assert_eq!(msg, Some("Pack tools: 5 available".to_string()));
    }

    #[test]
    fn test_pack_tools_message_mux_backend() {
        let msg = pack_tools_message(5, false);
        assert_eq!(
            msg,
            Some("Pack tools: 5 detected (mux TUI support pending)".to_string())
        );
    }

    #[test]
    fn test_pack_tools_message_zero_tools() {
        // Both backends should return None for zero tools
        assert_eq!(pack_tools_message(0, true), None);
        assert_eq!(pack_tools_message(0, false), None);
    }

    #[test]
    fn test_pack_tools_message_cli_says_available() {
        let msg = pack_tools_message(1, true).unwrap();
        assert!(
            msg.contains("available"),
            "CLI backend message should say 'available'"
        );
        assert!(
            !msg.contains("pending"),
            "CLI backend message should NOT say 'pending'"
        );
    }

    #[test]
    fn test_pack_tools_message_mux_says_pending() {
        let msg = pack_tools_message(1, false).unwrap();
        assert!(
            msg.contains("pending"),
            "Mux backend message should say 'pending'"
        );
        assert!(
            !msg.contains("available") || msg.contains("detected"),
            "Mux backend should not imply tools are usable"
        );
    }
}
