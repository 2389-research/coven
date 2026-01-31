// ABOUTME: Mux backend - uses mux-rs for native Rust agent execution.
// ABOUTME: Provides streaming LLM responses with SQLx session persistence.

use super::mux_tools::{
    WdBashTool, WdEditTool, WdListFilesTool, WdReadFileTool, WdSearchTool, WdWriteFileTool,
};
use super::{Backend, BackendEvent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use mux::prelude::*;
use mux::tool::Registry;
use mux::tools::{WebFetchTool, WebSearchTool};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Callback type for tool approval. Returns true if tool should execute.
pub type ApprovalCallback = Arc<
    dyn Fn(String, String, serde_json::Value) -> Pin<Box<dyn Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

/// Configuration for the Mux backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxConfig {
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,
    /// Maximum tokens for response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Path to global system prompt file (e.g., ~/.mux/system.md)
    pub global_system_prompt_path: Option<PathBuf>,
    /// Filenames to look for local system prompts
    #[serde(default = "default_local_prompt_files")]
    pub local_prompt_files: Vec<String>,
    /// Path to global soul.md (e.g., ~/.config/coven/soul.md)
    pub global_soul_path: Option<PathBuf>,
    /// Path to per-agent soul.md (absolute or relative to working_dir)
    pub agent_soul_path: Option<PathBuf>,
    /// Filenames to search for soul in working_dir (default: ["soul.md", ".coven/soul.md"])
    #[serde(default = "default_soul_files")]
    pub soul_files: Vec<String>,
    /// MCP servers to connect to (stdio transport)
    #[serde(default)]
    pub mcp_servers: Vec<MuxMcpServerConfig>,
    /// Skip registering default tools (read_file, write_file, bash, etc.)
    /// Useful for meta-agents that only need custom tools.
    #[serde(default)]
    pub skip_default_tools: bool,
    /// Gateway MCP endpoint for pack tools (HTTP transport).
    /// Set by the agent after connecting to the gateway.
    #[serde(skip)]
    pub gateway_mcp: Option<GatewayMcpConfig>,
}

/// Configuration for gateway MCP (pack tools) connection.
#[derive(Debug, Clone)]
pub struct GatewayMcpConfig {
    /// The MCP endpoint URL (e.g., "http://localhost:8080/mcp")
    pub url: String,
    /// Authentication token for the MCP endpoint
    pub token: String,
}

/// Configuration for an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxMcpServerConfig {
    /// Server name (used as tool prefix)
    pub name: String,
    /// Command to run
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_local_prompt_files() -> Vec<String> {
    vec![
        "claude.md".to_string(),
        "CLAUDE.md".to_string(),
        "agent.md".to_string(),
    ]
}

fn default_soul_files() -> Vec<String> {
    vec!["soul.md".to_string(), ".coven/soul.md".to_string()]
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: default_max_tokens(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            global_system_prompt_path: None,
            local_prompt_files: default_local_prompt_files(),
            global_soul_path: None,
            agent_soul_path: None,
            soul_files: default_soul_files(),
            mcp_servers: Vec::new(),
            skip_default_tools: false,
            gateway_mcp: None,
        }
    }
}

/// Maximum messages to keep in session history to prevent unbounded growth.
/// This limits memory usage and keeps API requests reasonably sized.
const MAX_SESSION_MESSAGES: usize = 200;

/// Session state with message history
#[derive(Clone)]
struct MuxSession {
    messages: Vec<Message>,
    system_prompt: Option<String>,
}

impl MuxSession {
    fn new(system_prompt: Option<String>) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt,
        }
    }

    /// Add a message to the session, pruning old messages if needed
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        // Keep only the most recent messages if we exceed the limit
        if self.messages.len() > MAX_SESSION_MESSAGES {
            // Remove oldest messages (keep the most recent ones)
            let to_remove = self.messages.len() - MAX_SESSION_MESSAGES;
            self.messages.drain(0..to_remove);
            tracing::debug!(
                removed = to_remove,
                remaining = self.messages.len(),
                "Pruned old messages from session"
            );
        }
    }
}

/// Serializable message format for SQLite storage
#[derive(Serialize, Deserialize)]
struct StoredMessage {
    role: String,
    content: Vec<StoredContentBlock>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum StoredContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// SQLx-backed session store for mux backend
struct SessionDb {
    pool: Pool<Sqlite>,
}

impl SessionDb {
    async fn new(db_path: &std::path::Path) -> Result<Self> {
        // Create parent directories if needed
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&db_url)
            .await
            .context("Failed to open mux sessions database")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS mux_sessions (
                session_id TEXT PRIMARY KEY,
                messages_json TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    async fn save_session(&self, session_id: &str, session: &MuxSession) -> Result<()> {
        let messages: Vec<StoredMessage> = session
            .messages
            .iter()
            .map(|m| StoredMessage {
                role: match m.role {
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m
                    .content
                    .iter()
                    .map(|c| match c {
                        ContentBlock::Text { text } => {
                            StoredContentBlock::Text { text: text.clone() }
                        }
                        ContentBlock::ToolUse { id, name, input } => StoredContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        },
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => StoredContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: *is_error,
                        },
                    })
                    .collect(),
            })
            .collect();

        let messages_json = serde_json::to_string(&messages)?;
        let now = now_unix_secs();

        sqlx::query(
            "INSERT INTO mux_sessions (session_id, messages_json, system_prompt, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
                messages_json = excluded.messages_json,
                system_prompt = excluded.system_prompt,
                updated_at = excluded.updated_at",
        )
        .bind(session_id)
        .bind(&messages_json)
        .bind(&session.system_prompt)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        tracing::debug!(session_id = %session_id, messages = messages.len(), "Session saved to database");
        Ok(())
    }

    async fn load_session(&self, session_id: &str) -> Result<Option<MuxSession>> {
        let result = sqlx::query(
            "SELECT messages_json, system_prompt FROM mux_sessions WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                let messages_json: String = row.get("messages_json");
                let system_prompt: Option<String> = row.get("system_prompt");

                let stored: Vec<StoredMessage> = serde_json::from_str(&messages_json)?;
                let messages: Vec<Message> = stored
                    .into_iter()
                    .map(|m| Message {
                        role: if m.role == "user" {
                            Role::User
                        } else {
                            Role::Assistant
                        },
                        content: m
                            .content
                            .into_iter()
                            .map(|c| match c {
                                StoredContentBlock::Text { text } => ContentBlock::Text { text },
                                StoredContentBlock::ToolUse { id, name, input } => {
                                    ContentBlock::ToolUse { id, name, input }
                                }
                                StoredContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                } => ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                },
                            })
                            .collect(),
                    })
                    .collect();

                tracing::debug!(session_id = %session_id, messages = messages.len(), "Session loaded from database");
                Ok(Some(MuxSession {
                    messages,
                    system_prompt,
                }))
            }
            None => Ok(None),
        }
    }
}

/// Default tools that require approval before execution
pub fn default_dangerous_tools() -> HashSet<String> {
    ["bash", "write_file", "edit"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Native Rust LLM backend using mux-rs
pub struct MuxBackend {
    config: MuxConfig,
    client: Arc<AnthropicClient>,
    session_db: Arc<SessionDb>,
    sessions: Arc<RwLock<HashMap<String, MuxSession>>>,
    registry: Arc<Registry>,
    /// Callback for tool approval. If Some, dangerous tools will wait for approval.
    approval_callback: Option<ApprovalCallback>,
    /// Set of tool names that require approval
    dangerous_tools: HashSet<String>,
}

impl MuxBackend {
    pub async fn new(config: MuxConfig) -> Result<Self> {
        // Create the Anthropic client
        let client =
            Arc::new(AnthropicClient::from_env().context("Failed to create Anthropic client")?);

        // Create session database in working directory
        let db_path = config.working_dir.join(".mux_sessions.db");
        let session_db = Arc::new(SessionDb::new(&db_path).await?);

        // Create and populate tool registry
        let registry = Arc::new(Registry::new());
        let wd = config.working_dir.clone();

        // Register built-in tools (unless skipped for meta-agents)
        if !config.skip_default_tools {
            registry.register(WdReadFileTool::new(wd.clone())).await;
            registry.register(WdWriteFileTool::new(wd.clone())).await;
            registry.register(WdEditTool::new(wd.clone())).await;
            registry.register(WdBashTool::new(wd.clone())).await;
            registry.register(WdListFilesTool::new(wd.clone())).await;
            registry.register(WdSearchTool::new(wd.clone())).await;
            registry.register(WebFetchTool::new()).await;
            registry.register(WebSearchTool::new()).await;

            tracing::info!(
                working_dir = %wd.display(),
                "Registered 8 built-in tools: read_file, write_file, edit, bash, list_files, search, web_fetch, web_search"
            );
        } else {
            tracing::info!("Skipped default tools (meta-agent mode)");
        }

        // Connect stdio MCP servers (background, don't block)
        let registry_clone = Arc::clone(&registry);
        let mcp_configs = config.mcp_servers.clone();
        tokio::spawn(async move {
            for server_config in mcp_configs {
                if let Err(e) = connect_mcp_server(&server_config, &registry_clone).await {
                    tracing::error!(
                        server = %server_config.name,
                        error = %e,
                        "Failed to connect MCP server"
                    );
                } else {
                    tracing::info!(server = %server_config.name, "MCP server connected");
                }
            }
        });

        // Connect gateway MCP for pack tools (background, don't block)
        if let Some(ref gw_mcp) = config.gateway_mcp {
            let registry_clone = Arc::clone(&registry);
            let url = gw_mcp.url.clone();
            let token = gw_mcp.token.clone();
            tokio::spawn(async move {
                match crate::mcp_http::connect_gateway_mcp(
                    &registry_clone,
                    &url,
                    &token,
                    Some("pack"),
                )
                .await
                {
                    Ok(count) => {
                        tracing::info!(
                            tools = count,
                            "Connected to gateway MCP - registered {} pack tools",
                            count
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to connect to gateway MCP for pack tools");
                    }
                }
            });
        }

        Ok(Self {
            config,
            client,
            session_db,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            registry,
            approval_callback: None,
            dangerous_tools: default_dangerous_tools(),
        })
    }

    /// Set the approval callback for dangerous tools.
    /// When set, tools in the dangerous_tools set will emit ToolApprovalRequest
    /// and wait for approval before executing.
    pub fn set_approval_callback(&mut self, callback: ApprovalCallback) {
        self.approval_callback = Some(callback);
    }

    /// Set the approval callback (builder pattern).
    pub fn with_approval_callback(mut self, callback: ApprovalCallback) -> Self {
        self.approval_callback = Some(callback);
        self
    }

    /// Override the set of dangerous tools that require approval.
    pub fn with_dangerous_tools(mut self, tools: HashSet<String>) -> Self {
        self.dangerous_tools = tools;
        self
    }

    /// Register an additional tool with the backend's registry.
    /// This allows injecting custom tools after backend construction.
    pub async fn register_tool<T: mux::tool::Tool + Send + Sync + 'static>(&self, tool: T) {
        self.registry.register(tool).await;
    }

    /// Connect to gateway MCP endpoint to access pack tools.
    /// This should be called after the gateway welcome message is received.
    /// Returns the number of pack tools registered.
    pub async fn connect_gateway_mcp(&self, url: &str, token: &str) -> Result<usize> {
        let count =
            crate::mcp_http::connect_gateway_mcp(&self.registry, url, token, Some("pack")).await?;
        tracing::info!(
            tools = count,
            url = %url,
            "Connected to gateway MCP - registered {} pack tools",
            count
        );
        Ok(count)
    }
}

#[async_trait]
impl Backend for MuxBackend {
    fn name(&self) -> &'static str {
        "mux"
    }

    async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        // Load or create session
        if is_new_session {
            let system_prompt = build_system_prompt(&self.config);
            let session = MuxSession::new(system_prompt);
            self.session_db.save_session(session_id, &session).await?;
            self.sessions
                .write()
                .await
                .insert(session_id.to_string(), session);
            let _ = tx
                .send(BackendEvent::SessionInit {
                    session_id: session_id.to_string(),
                })
                .await;
        } else {
            // Try to load from cache or database
            // Hold write lock during entire check-and-insert to avoid TOCTOU race
            let mut sessions_guard = self.sessions.write().await;
            if !sessions_guard.contains_key(session_id) {
                match self.session_db.load_session(session_id).await? {
                    Some(session) => {
                        sessions_guard.insert(session_id.to_string(), session);
                    }
                    None => {
                        // Session doesn't exist - signal orphan
                        drop(sessions_guard);
                        let _ = tx.send(BackendEvent::SessionOrphaned).await;
                        drop(tx);
                        return Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)));
                    }
                }
            }
        }

        // Clone data for the async task
        let client = Arc::clone(&self.client);
        let sessions = Arc::clone(&self.sessions);
        let session_db = Arc::clone(&self.session_db);
        let registry = Arc::clone(&self.registry);
        let config = self.config.clone();
        let session_id = session_id.to_string();
        let message = message.to_string();
        let approval_callback = self.approval_callback.clone();
        let dangerous_tools = self.dangerous_tools.clone();

        tokio::spawn(async move {
            if let Err(e) = run_prompt(
                &client,
                &sessions,
                &session_db,
                &registry,
                &config,
                &session_id,
                &message,
                tx,
                approval_callback,
                &dangerous_tools,
            )
            .await
            {
                tracing::error!(error = %e, "Mux prompt failed");
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// Build system prompt from global and local files, including soul files for identity/personality.
///
/// Prompt building order:
/// 1. Working directory context (always first)
/// 2. Global system prompt (~/.mux/system.md or configured path)
/// 3. Global soul (~/.config/coven/soul.md or configured path)
/// 4. Per-agent soul (.coven/soul.md or configured path in working_dir)
/// 5. Local project prompt (claude.md, CLAUDE.md, etc.)
fn build_system_prompt(config: &MuxConfig) -> Option<String> {
    let mut parts = Vec::new();

    // 1. Working directory context - critical for tools to work correctly
    let working_dir_str = config.working_dir.to_string_lossy();
    parts.push(format!(
        "# Environment\n\n\
        Your working directory is: {}\n\n\
        When using file tools (read_file, write_file, list_files, search), paths are relative to this directory.\n\
        When using the bash tool, pass working_dir: \"{}\" unless you need to run in a different directory.",
        working_dir_str, working_dir_str
    ));

    // 2. Global system prompt (~/.mux/system.md or configured path)
    if let Some(ref global_path) = config.global_system_prompt_path {
        if let Ok(content) = std::fs::read_to_string(global_path) {
            if !content.trim().is_empty() {
                parts.push(content);
            }
        }
    } else if let Some(home) = home_dir() {
        // Default to ~/.mux/system.md
        let default_path = home.join(".mux").join("system.md");
        if let Ok(content) = std::fs::read_to_string(default_path) {
            if !content.trim().is_empty() {
                parts.push(content);
            }
        }
    }

    // 3. Global soul (~/.config/coven/soul.md or configured path)
    let global_soul_content = if let Some(ref soul_path) = config.global_soul_path {
        // Expand tilde in configured path
        let expanded = expand_tilde(soul_path);
        std::fs::read_to_string(&expanded).ok()
    } else if let Some(home) = home_dir() {
        // Default to ~/.config/coven/soul.md
        let default_soul_path = home.join(".config").join("coven").join("soul.md");
        std::fs::read_to_string(default_soul_path).ok()
    } else {
        None
    };

    if let Some(content) = global_soul_content {
        if !content.trim().is_empty() {
            parts.push(format!("# Identity\n\n{}", content));
        }
    }

    // 4. Per-agent soul (configured path or search in working_dir)
    let agent_soul_content = if let Some(ref soul_path) = config.agent_soul_path {
        // Expand tilde, then resolve relative paths against working_dir
        let expanded = expand_tilde(soul_path);
        let resolved = if expanded.is_absolute() {
            expanded
        } else {
            config.working_dir.join(expanded)
        };
        std::fs::read_to_string(resolved).ok()
    } else {
        // Search soul_files in working_dir
        config.soul_files.iter().find_map(|filename| {
            let path = config.working_dir.join(filename);
            std::fs::read_to_string(&path).ok()
        })
    };

    if let Some(content) = agent_soul_content {
        if !content.trim().is_empty() {
            // If no global soul was added, add the Identity header
            let needs_header = !parts.iter().any(|p| p.starts_with("# Identity"));
            if needs_header {
                parts.push(format!("# Identity\n\n{}", content));
            } else {
                // Append to existing identity section with separator
                parts.push(format!("## Agent Identity\n\n{}", content));
            }
        }
    }

    // 5. Local system prompt (claude.md, agent.md, etc. in working_dir)
    for filename in &config.local_prompt_files {
        let local_path = config.working_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&local_path) {
            if !content.trim().is_empty() {
                parts.push(content);
                break;
            }
        }
    }

    // Always have at least the working directory context
    Some(parts.join("\n\n---\n\n"))
}

/// Connect to an MCP server and register its tools
async fn connect_mcp_server(config: &MuxMcpServerConfig, registry: &Registry) -> Result<()> {
    use mux::mcp::{McpClient, McpServerConfig, McpTransport};

    let mcp_config = McpServerConfig {
        name: config.name.clone(),
        transport: McpTransport::Stdio {
            command: config.command.clone(),
            args: config.args.clone(),
            env: config.env.clone(),
        },
    };

    let mut client = McpClient::connect(mcp_config).await?;
    client.initialize().await?;

    let client = Arc::new(client);
    let tool_count = registry
        .merge_mcp(Arc::clone(&client), Some(&config.name))
        .await?;

    tracing::info!(
        server = %config.name,
        tools = tool_count,
        "Registered MCP tools"
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_prompt(
    client: &AnthropicClient,
    sessions: &Arc<RwLock<HashMap<String, MuxSession>>>,
    session_db: &Arc<SessionDb>,
    registry: &Registry,
    config: &MuxConfig,
    session_id: &str,
    text: &str,
    event_tx: tokio::sync::mpsc::Sender<BackendEvent>,
    approval_callback: Option<ApprovalCallback>,
    dangerous_tools: &HashSet<String>,
) -> Result<()> {
    // Get tool definitions from registry
    let tools = registry.to_definitions().await;

    // Get session and add user message
    let system_prompt = {
        let mut sessions_guard = sessions.write().await;
        let session = sessions_guard
            .get_mut(session_id)
            .context("Session not found")?;

        session.add_message(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        });

        session.system_prompt.clone()
    };

    let mut accumulated_text = String::new();

    // Emit thinking event at start
    let _ = event_tx.send(BackendEvent::Thinking).await;

    // Agentic loop - continues while LLM requests tool use
    // Limited to prevent infinite loops if LLM keeps requesting tools
    const MAX_ITERATIONS: usize = 50;
    let mut iteration = 0;

    loop {
        iteration += 1;
        if iteration > MAX_ITERATIONS {
            tracing::warn!(
                session_id = %session_id,
                iterations = iteration,
                "Agentic loop exceeded max iterations, stopping"
            );
            let _ = event_tx
                .send(BackendEvent::Error(
                    "Agent exceeded maximum tool iterations (50). Stopping to prevent infinite loop.".to_string()
                ))
                .await;
            break;
        }
        // Get current messages from session
        let messages = {
            let sessions_guard = sessions.read().await;
            let session = sessions_guard
                .get(session_id)
                .context("Session not found")?;
            session.messages.clone()
        };

        // Build request
        let request = Request {
            model: config.model.clone(),
            messages,
            tools: tools.clone(),
            max_tokens: Some(config.max_tokens),
            system: system_prompt.clone(),
            temperature: None,
        };

        // Use streaming API for real-time text output
        let mut stream = client.create_message_stream(&request);
        let mut response_content: Vec<ContentBlock> = Vec::new();
        let mut current_text = String::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut stop_reason: Option<StopReason> = None;

        // Track tool input JSON accumulation by block index
        let mut tool_input_accum: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        let mut tool_index_map: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => match &event {
                    StreamEvent::ContentBlockDelta { text, .. } => {
                        current_text.push_str(text);
                        accumulated_text.push_str(text);
                        if event_tx
                            .send(BackendEvent::Text(text.clone()))
                            .await
                            .is_err()
                        {
                            tracing::debug!("Event receiver closed, stopping stream");
                            return Ok(());
                        }
                    }
                    StreamEvent::InputJsonDelta {
                        index,
                        partial_json,
                    } => {
                        tool_input_accum
                            .entry(*index)
                            .or_default()
                            .push_str(partial_json);
                    }
                    StreamEvent::ContentBlockStart { index, block } => {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            let _ = event_tx
                                .send(BackendEvent::ToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                })
                                .await;
                            tool_index_map.insert(*index, tool_uses.len());
                            tool_uses.push((id.clone(), name.clone(), input.clone()));
                        }
                    }
                    StreamEvent::ContentBlockStop { index } => {
                        if !current_text.is_empty() && response_content.len() == *index {
                            response_content.push(ContentBlock::Text {
                                text: std::mem::take(&mut current_text),
                            });
                        }
                        if let Some(tool_idx) = tool_index_map.get(index) {
                            if let Some(json_str) = tool_input_accum.remove(index) {
                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                    if let Some(tool) = tool_uses.get_mut(*tool_idx) {
                                        tool.2 = parsed;
                                    }
                                }
                            }
                        }
                    }
                    StreamEvent::MessageDelta {
                        stop_reason: sr,
                        usage,
                    } => {
                        stop_reason = *sr;
                        // Emit usage event if we have non-zero usage data
                        if usage.input_tokens > 0 || usage.output_tokens > 0 {
                            let _ = event_tx
                                .send(BackendEvent::Usage {
                                    input_tokens: usage.input_tokens as i32,
                                    output_tokens: usage.output_tokens as i32,
                                    cache_read_tokens: usage.cache_read_tokens as i32,
                                    cache_write_tokens: usage.cache_write_tokens as i32,
                                    thinking_tokens: 0, // mux crate doesn't track this yet
                                })
                                .await;
                        }
                    }
                    StreamEvent::MessageStart { .. } | StreamEvent::MessageStop => {}
                },
                Err(e) => {
                    let _ = event_tx
                        .send(BackendEvent::Error(format!("LLM error: {}", e)))
                        .await;
                    let _ = event_tx
                        .send(BackendEvent::Done {
                            full_response: accumulated_text.clone(),
                        })
                        .await;
                    return Err(e.into());
                }
            }
        }

        // Add any remaining text
        if !current_text.is_empty() {
            response_content.push(ContentBlock::Text { text: current_text });
        }

        // Add tool uses to content
        for (id, name, input) in &tool_uses {
            response_content.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }

        // Update session with assistant response
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(session) = sessions_guard.get_mut(session_id) {
                if !response_content.is_empty() {
                    session.add_message(Message {
                        role: Role::Assistant,
                        content: response_content.clone(),
                    });
                }
            }
        }

        // If no tool use, we're done
        if stop_reason != Some(StopReason::ToolUse) || tool_uses.is_empty() {
            // Persist final state
            {
                let sessions_guard = sessions.read().await;
                if let Some(session) = sessions_guard.get(session_id) {
                    if let Err(e) = session_db.save_session(session_id, session).await {
                        tracing::error!(error = %e, "Failed to persist session after prompt");
                    }
                }
            }
            break;
        }

        // Execute tools and collect results
        let mut tool_results: Vec<ContentBlock> = Vec::new();

        for (tool_id, tool_name, tool_input) in tool_uses {
            // Check if this tool needs approval
            if let Some(ref callback) = approval_callback {
                if dangerous_tools.contains(&tool_name) {
                    // Emit approval request event
                    let _ = event_tx
                        .send(BackendEvent::ToolApprovalRequest {
                            id: tool_id.clone(),
                            name: tool_name.clone(),
                            input: tool_input.clone(),
                        })
                        .await;

                    // Wait for approval
                    let approved =
                        callback(tool_id.clone(), tool_name.clone(), tool_input.clone()).await;

                    if !approved {
                        tracing::info!(tool = %tool_name, "Tool execution denied by user");

                        // Emit denial as tool result
                        let output = "Tool execution denied by user".to_string();
                        let _ = event_tx
                            .send(BackendEvent::ToolResult {
                                id: tool_id.clone(),
                                output: output.clone(),
                                is_error: true,
                            })
                            .await;

                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: tool_id,
                            content: output,
                            is_error: true,
                        });
                        continue;
                    }

                    tracing::info!(tool = %tool_name, "Tool execution approved by user");
                }
            }

            let start_time = Instant::now();

            // Look up and execute the tool
            let (output, is_error) = if let Some(tool) = registry.get(&tool_name).await {
                match tool.execute(tool_input.clone()).await {
                    Ok(result) => (result.content, result.is_error),
                    Err(e) => (format!("Tool execution error: {}", e), true),
                }
            } else {
                (format!("Tool '{}' not found in registry", tool_name), true)
            };

            let duration_ms = start_time.elapsed().as_millis() as u64;
            tracing::debug!(
                tool = %tool_name,
                duration_ms = duration_ms,
                is_error = is_error,
                "Tool executed"
            );

            // Emit tool result event
            let _ = event_tx
                .send(BackendEvent::ToolResult {
                    id: tool_id.clone(),
                    output: output.clone(),
                    is_error,
                })
                .await;

            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: output,
                is_error,
            });
        }

        // Add tool results as a user message and persist
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(session) = sessions_guard.get_mut(session_id) {
                session.add_message(Message {
                    role: Role::User,
                    content: tool_results,
                });

                // Persist after tool execution
                if let Err(e) = session_db.save_session(session_id, session).await {
                    tracing::error!(error = %e, "Failed to persist session after tool execution");
                }
            }
        }

        // Emit thinking for next iteration
        let _ = event_tx.send(BackendEvent::Thinking).await;
    }

    // Send done event
    let _ = event_tx
        .send(BackendEvent::Done {
            full_response: accumulated_text,
        })
        .await;

    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Expand tilde (~) in a path to the user's home directory.
fn expand_tilde(path: &std::path::Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    PathBuf::from(shellexpand::tilde(&path_str).into_owned())
}

fn now_unix_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_system_prompt_includes_working_dir() {
        let temp_dir = TempDir::new().unwrap();
        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        assert!(prompt.contains("# Environment"));
        assert!(prompt.contains(&temp_dir.path().to_string_lossy().to_string()));
    }

    #[test]
    fn test_build_system_prompt_loads_global_soul() {
        let temp_dir = TempDir::new().unwrap();
        let soul_path = temp_dir.path().join("soul.md");
        std::fs::write(&soul_path, "You are Aster, a helpful agent.").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            global_soul_path: Some(soul_path),
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("You are Aster, a helpful agent."));
    }

    #[test]
    fn test_build_system_prompt_loads_agent_soul_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let agent_soul_path = temp_dir.path().join("agent-soul.md");
        std::fs::write(&agent_soul_path, "Agent-specific personality traits.").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            agent_soul_path: Some(agent_soul_path),
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("Agent-specific personality traits."));
    }

    #[test]
    fn test_build_system_prompt_searches_soul_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create .coven/soul.md in working_dir
        let coven_dir = temp_dir.path().join(".coven");
        std::fs::create_dir_all(&coven_dir).unwrap();
        std::fs::write(coven_dir.join("soul.md"), "Found via soul_files search.").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            soul_files: vec![".coven/soul.md".to_string()],
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("Found via soul_files search."));
    }

    #[test]
    fn test_build_system_prompt_agent_soul_relative_path() {
        let temp_dir = TempDir::new().unwrap();

        // Create agent soul at a relative path within working_dir
        let subdir = temp_dir.path().join("config");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join("my-soul.md"), "Relative path agent soul.").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            // Relative path should be resolved against working_dir
            agent_soul_path: Some(PathBuf::from("config/my-soul.md")),
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("Relative path agent soul."));
    }

    #[test]
    fn test_build_system_prompt_both_global_and_agent_soul() {
        let temp_dir = TempDir::new().unwrap();

        // Create global soul
        let global_soul_path = temp_dir.path().join("global-soul.md");
        std::fs::write(&global_soul_path, "Global identity content.").unwrap();

        // Create agent soul in working_dir
        let agent_soul_path = temp_dir.path().join("soul.md");
        std::fs::write(&agent_soul_path, "Agent-specific content.").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            global_soul_path: Some(global_soul_path),
            soul_files: vec!["soul.md".to_string()],
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        // Both should be included
        assert!(prompt.contains("Global identity content."));
        assert!(prompt.contains("Agent-specific content."));
        // Global soul gets # Identity, agent soul gets ## Agent Identity
        assert!(prompt.contains("# Identity"));
        assert!(prompt.contains("## Agent Identity"));
    }

    #[test]
    fn test_build_system_prompt_order() {
        let temp_dir = TempDir::new().unwrap();

        // Create global system prompt
        let system_prompt_path = temp_dir.path().join("system.md");
        std::fs::write(&system_prompt_path, "GLOBAL_SYSTEM_MARKER").unwrap();

        // Create global soul
        let global_soul_path = temp_dir.path().join("soul.md");
        std::fs::write(&global_soul_path, "GLOBAL_SOUL_MARKER").unwrap();

        // Create local project prompt
        std::fs::write(temp_dir.path().join("claude.md"), "LOCAL_PROMPT_MARKER").unwrap();

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            global_system_prompt_path: Some(system_prompt_path),
            global_soul_path: Some(global_soul_path),
            local_prompt_files: vec!["claude.md".to_string()],
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        // Verify order: Environment → Global System → Global Soul → Local Prompt
        let env_pos = prompt.find("# Environment").unwrap();
        let system_pos = prompt.find("GLOBAL_SYSTEM_MARKER").unwrap();
        let soul_pos = prompt.find("GLOBAL_SOUL_MARKER").unwrap();
        let local_pos = prompt.find("LOCAL_PROMPT_MARKER").unwrap();

        assert!(
            env_pos < system_pos,
            "Environment should come before global system"
        );
        assert!(
            system_pos < soul_pos,
            "Global system should come before global soul"
        );
        assert!(soul_pos < local_pos, "Global soul should come before local prompt");
    }

    #[test]
    fn test_build_system_prompt_empty_soul_ignored() {
        let temp_dir = TempDir::new().unwrap();

        // Create empty soul file
        let soul_path = temp_dir.path().join("soul.md");
        std::fs::write(&soul_path, "   \n  \n").unwrap(); // Whitespace only

        let config = MuxConfig {
            working_dir: temp_dir.path().to_path_buf(),
            global_soul_path: Some(soul_path),
            ..MuxConfig::default()
        };

        let prompt = build_system_prompt(&config).unwrap();

        // Empty soul should not add Identity section
        assert!(!prompt.contains("# Identity"));
    }

    #[test]
    fn test_default_soul_files() {
        let files = default_soul_files();
        assert_eq!(files, vec!["soul.md", ".coven/soul.md"]);
    }

    #[test]
    fn test_mux_config_default_includes_soul_fields() {
        let config = MuxConfig::default();

        assert!(config.global_soul_path.is_none());
        assert!(config.agent_soul_path.is_none());
        assert_eq!(config.soul_files, vec!["soul.md", ".coven/soul.md"]);
    }
}
