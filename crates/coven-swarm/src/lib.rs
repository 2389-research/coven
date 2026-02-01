// ABOUTME: coven-swarm library with supervisor, agent, and init modules.
// ABOUTME: Re-exports for programmatic use of swarm functionality.

pub mod agent;
pub mod init;
pub mod supervisor;

pub use agent::{GatewayClient, Session};
pub use coven_swarm_core::Config;
pub use init::run_init;
pub use supervisor::{
    discover_workspaces, socket, AgentProcess, AgentStatus, SocketClient, SocketCommand,
    StatusInfo, Tui, TuiEvent,
};

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Options for running the supervisor
pub struct SupervisorOptions {
    /// Path to configuration file
    pub config_path: Option<PathBuf>,
    /// Run in headless mode (no TUI)
    pub headless: bool,
}

/// Options for running a swarm agent (internal, spawned by supervisor)
pub struct AgentOptions {
    /// Workspace name
    pub workspace: String,
    /// Run in dispatch mode (swarm management tools)
    pub dispatch_mode: bool,
    /// Path to configuration file
    pub config_path: Option<PathBuf>,
}

/// Run the supervisor daemon
pub async fn run_supervisor(options: SupervisorOptions) -> Result<()> {
    let config_path = options
        .config_path
        .unwrap_or_else(|| Config::default_path().expect("Failed to get default config path"));
    let config = Config::load(&config_path)?;
    let working_dir = config.working_directory_expanded();

    // Create TUI if not headless
    let tui_tx: Option<mpsc::Sender<TuiEvent>> = if options.headless {
        None
    } else {
        match Tui::new() {
            Ok((tui, tx)) => {
                // Run TUI in background
                tokio::spawn(async move {
                    if let Err(e) = tui.run().await {
                        eprintln!("TUI error: {}", e);
                    }
                    // TUI quit, exit the process
                    std::process::exit(0);
                });
                Some(tx)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create TUI, falling back to headless");
                None
            }
        }
    };

    // Helper to send TUI events or log
    let send_event = |tx: &Option<mpsc::Sender<TuiEvent>>, event: TuiEvent| {
        if let Some(tx) = tx {
            let _ = tx.try_send(event);
        }
    };

    // Discover existing workspaces
    let workspaces = discover_workspaces(&working_dir)?;
    if let Some(ref tx) = tui_tx {
        let _ = tx.try_send(TuiEvent::System {
            message: format!("Discovered {} workspaces", workspaces.len()),
        });
    } else {
        tracing::info!(count = workspaces.len(), "Discovered workspaces");
    }

    // Spawn agents
    let mut agents: HashMap<String, AgentProcess> = HashMap::new();

    for workspace in workspaces {
        // dispatch workspace gets dispatch_mode=true
        let dispatch_mode = workspace == "dispatch";
        let mut agent = AgentProcess::new(workspace.clone(), config_path.clone(), dispatch_mode);
        agent.spawn_with_tui(tui_tx.clone()).await?;
        if let Some(ref tx) = tui_tx {
            let _ = tx.try_send(TuiEvent::AgentSpawned {
                workspace: workspace.clone(),
                pid: agent.pid().unwrap_or(0),
            });
        }
        agents.insert(workspace, agent);
    }

    // Ensure dispatch exists (create if not discovered)
    if !agents.contains_key("dispatch") {
        std::fs::create_dir_all(working_dir.join("dispatch"))?;
        let mut dispatch = AgentProcess::new("dispatch".to_string(), config_path.clone(), true);
        dispatch.spawn_with_tui(tui_tx.clone()).await?;
        if let Some(ref tx) = tui_tx {
            let _ = tx.try_send(TuiEvent::AgentSpawned {
                workspace: "dispatch".to_string(),
                pid: dispatch.pid().unwrap_or(0),
            });
        }
        agents.insert("dispatch".to_string(), dispatch);
    }

    // Start socket server
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(32);
    let socket_path = socket::socket_path(&config.prefix);
    let prefix_for_status = config.prefix.clone();

    let tui_tx_socket = tui_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = socket::run_socket_server(socket_path, cmd_tx).await {
            if let Some(ref tx) = tui_tx_socket {
                let _ = tx.try_send(TuiEvent::System {
                    message: format!("Socket server error: {}", e),
                });
            } else {
                tracing::error!(error = %e, "Socket server error");
            }
        }
    });

    if let Some(ref tx) = tui_tx {
        let _ = tx.try_send(TuiEvent::System {
            message: "Socket server listening".to_string(),
        });
    }

    // Handle socket commands
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            SocketCommand::List { reply } => {
                send_event(
                    &tui_tx,
                    TuiEvent::SocketCommand {
                        command: "list".to_string(),
                    },
                );
                let names: Vec<String> = agents.keys().cloned().collect();
                let _ = reply.send(names);
            }
            SocketCommand::Create { name, reply } => {
                send_event(
                    &tui_tx,
                    TuiEvent::SocketCommand {
                        command: format!("create {}", name),
                    },
                );
                if agents.contains_key(&name) {
                    let _ = reply.send(Err(anyhow::anyhow!("Workspace already exists")));
                    continue;
                }
                std::fs::create_dir_all(working_dir.join(&name))?;
                let mut agent = AgentProcess::new(name.clone(), config_path.clone(), false);
                if let Err(e) = agent.spawn_with_tui(tui_tx.clone()).await {
                    let _ = reply.send(Err(e));
                    continue;
                }
                if let Some(ref tx) = tui_tx {
                    let _ = tx.try_send(TuiEvent::AgentSpawned {
                        workspace: name.clone(),
                        pid: agent.pid().unwrap_or(0),
                    });
                }
                let agent_id = format!("{}_{}", config.prefix, name);
                agents.insert(name, agent);
                let _ = reply.send(Ok(agent_id));
            }
            SocketCommand::Delete { name, reply } => {
                send_event(
                    &tui_tx,
                    TuiEvent::SocketCommand {
                        command: format!("delete {}", name),
                    },
                );
                if let Some(mut agent) = agents.remove(&name) {
                    let _ = agent.kill().await;
                    if let Some(ref tx) = tui_tx {
                        let _ = tx.try_send(TuiEvent::AgentExited {
                            workspace: name,
                            code: Some(0),
                        });
                    }
                    let _ = reply.send(Ok(()));
                } else {
                    let _ = reply.send(Err(anyhow::anyhow!("Workspace not found")));
                }
            }
            SocketCommand::Status { reply } => {
                send_event(
                    &tui_tx,
                    TuiEvent::SocketCommand {
                        command: "status".to_string(),
                    },
                );
                let agent_statuses: Vec<AgentStatus> = agents
                    .iter()
                    .map(|(name, agent)| AgentStatus {
                        workspace: name.clone(),
                        pid: agent.pid(),
                        running: agent.is_running(),
                    })
                    .collect();
                let status_info = StatusInfo {
                    prefix: prefix_for_status.clone(),
                    agents: agent_statuses,
                };
                let _ = reply.send(status_info);
            }
            SocketCommand::Stop { reply } => {
                send_event(
                    &tui_tx,
                    TuiEvent::SocketCommand {
                        command: "stop".to_string(),
                    },
                );
                // Kill all agents
                for (name, mut agent) in agents.drain() {
                    let _ = agent.kill().await;
                    if let Some(ref tx) = tui_tx {
                        let _ = tx.try_send(TuiEvent::AgentExited {
                            workspace: name,
                            code: Some(0),
                        });
                    }
                }
                let _ = reply.send(());
                // Exit the supervisor loop
                break;
            }
        }
    }

    Ok(())
}

/// Run a swarm agent (internal, spawned by supervisor)
pub async fn run_agent(options: AgentOptions) -> Result<()> {
    use coven_core::backend::{DirectCliBackend, DirectCliConfig, MuxBackend, MuxConfig};
    use coven_swarm_backend::dispatch_tools::{
        CreateWorkspaceTool, DeleteWorkspaceTool, ListAgentsTool,
    };
    use coven_swarm_backend::BackendHandle;
    use coven_swarm_core::BackendType;

    use crate::agent::{new_pending_pack_tools, PackTool};

    // Load or generate SSH key
    let key_path = coven_ssh::default_swarm_key_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine SSH key path"))?;
    let private_key = coven_ssh::load_or_generate_key(&key_path)?;
    let fingerprint = coven_ssh::compute_fingerprint(private_key.public_key())?;
    tracing::info!(fingerprint = %fingerprint, "SSH key loaded");

    // Load config
    let config_path = options
        .config_path
        .unwrap_or_else(|| Config::default_path().expect("Failed to get default config path"));
    let config = Config::load(&config_path)?;

    let working_dir = config.working_directory_expanded().join(&options.workspace);

    // Validate working directory exists
    if !working_dir.exists() {
        anyhow::bail!(
            "Workspace directory does not exist: {}",
            working_dir.display()
        );
    }

    // Track concrete backend types for pack tool setup
    // We keep Arc references so we can configure them in the on_welcome callback
    let mut mux_backend: Option<Arc<MuxBackend>> = None;
    let mut cli_backend: Option<Arc<DirectCliBackend>> = None;

    // Create backend based on mode
    let (handle, backend_name) = if options.dispatch_mode {
        // Dispatch mode uses coven-core's MuxBackend with dispatch tools
        let mux_config = MuxConfig {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 8192,
            working_dir: working_dir.clone(),
            global_system_prompt_path: None,
            local_prompt_files: vec!["claude.md".to_string(), "CLAUDE.md".to_string()],
            global_soul_path: config.global_soul_path.as_ref().map(PathBuf::from),
            agent_soul_path: config.dispatch_soul_path.as_ref().map(PathBuf::from),
            soul_files: config.soul_files.clone(),
            skip_default_tools: false,
            ..MuxConfig::default()
        };
        let backend = Arc::new(MuxBackend::new(mux_config).await?);

        // Register dispatch tools
        backend
            .register_tool(ListAgentsTool::new(config.prefix.clone()))
            .await;
        backend
            .register_tool(CreateWorkspaceTool::new(config.prefix.clone()))
            .await;
        backend
            .register_tool(DeleteWorkspaceTool::new(config.prefix.clone()))
            .await;
        tracing::info!(
            "Registered dispatch tools: list_agents, create_workspace, delete_workspace"
        );

        mux_backend = Some(backend.clone());
        let handle = BackendHandle::new_from_arc(backend);
        let name = handle.name();
        (handle, name)
    } else {
        // Normal workspace - use backend from config
        match config.default_backend {
            BackendType::Direct => {
                // DirectCliBackend spawns Claude CLI subprocess
                let cli_config = DirectCliConfig {
                    binary: config.acp_binary.clone(), // reuse acp_binary setting
                    working_dir: working_dir.clone(),
                    timeout_secs: 300,
                    mcp_endpoint: None, // Set after receiving Welcome with mcp_token
                };
                let backend = Arc::new(DirectCliBackend::new(cli_config));
                cli_backend = Some(backend.clone());
                let handle = BackendHandle::new_from_arc(backend);
                let name = handle.name();
                (handle, name)
            }
            BackendType::Mux => {
                // MuxBackend uses Anthropic API directly
                let mux_config = MuxConfig {
                    model: "claude-sonnet-4-20250514".to_string(),
                    max_tokens: 8192,
                    working_dir: working_dir.clone(),
                    global_system_prompt_path: None,
                    local_prompt_files: vec!["claude.md".to_string(), "CLAUDE.md".to_string()],
                    global_soul_path: config.global_soul_path.as_ref().map(PathBuf::from),
                    agent_soul_path: None, // Per-agent soul loaded from working_dir
                    soul_files: config.soul_files.clone(),
                    skip_default_tools: false,
                    ..MuxConfig::default()
                };
                let backend = Arc::new(MuxBackend::new(mux_config).await?);
                mux_backend = Some(backend.clone());
                let handle = BackendHandle::new_from_arc(backend);
                let name = handle.name();
                (handle, name)
            }
            BackendType::Acp => {
                // AcpBackend uses Claude's Agent Computer Protocol
                #[cfg(feature = "acp")]
                {
                    use coven_swarm_backend::acp::{AcpBackend, AcpConfig};
                    let acp_config = AcpConfig {
                        binary: config.acp_binary.clone(),
                        timeout_secs: 300,
                        working_dir: working_dir.clone(),
                        extra_args: vec![],
                    };
                    let backend = AcpBackend::new(acp_config);
                    let handle = BackendHandle::new(backend);
                    let name = handle.name();
                    (handle, name)
                }
                #[cfg(not(feature = "acp"))]
                {
                    // Fallback to MuxBackend when ACP feature is not enabled
                    tracing::warn!(
                        "ACP backend requested but feature not enabled, falling back to Mux"
                    );
                    let mux_config = MuxConfig {
                        model: "claude-sonnet-4-20250514".to_string(),
                        max_tokens: 8192,
                        working_dir: working_dir.clone(),
                        global_system_prompt_path: None,
                        local_prompt_files: vec!["claude.md".to_string(), "CLAUDE.md".to_string()],
                        global_soul_path: config.global_soul_path.as_ref().map(PathBuf::from),
                        agent_soul_path: None, // Per-agent soul loaded from working_dir
                        soul_files: config.soul_files.clone(),
                        skip_default_tools: false,
                        ..MuxConfig::default()
                    };
                    let backend = Arc::new(MuxBackend::new(mux_config).await?);
                    mux_backend = Some(backend.clone());
                    let handle = BackendHandle::new_from_arc(backend);
                    let name = handle.name();
                    (handle, name)
                }
            }
        }
    };

    // Create pending pack tools registry for mux backends (gRPC-routed pack tools)
    let pending_pack_tools = if mux_backend.is_some() {
        Some(new_pending_pack_tools())
    } else {
        None
    };

    let session = Arc::new(Mutex::new(Session::new(handle)));

    let gateway_url = config.gateway_url()?;

    tracing::info!(
        workspace = %options.workspace,
        working_dir = %working_dir.display(),
        gateway = %gateway_url,
        backend = %backend_name,
        dispatch_mode = %options.dispatch_mode,
        "Starting agent"
    );

    // Connect to gateway
    let client = GatewayClient::connect(
        &gateway_url,
        &config.prefix,
        &options.workspace,
        &working_dir.to_string_lossy(),
        backend_name,
    )
    .await?;

    // Run the agent with pack tool support
    let pending_for_callback = pending_pack_tools.clone();
    client
        .run_with_pack_tools(
            |msg, tx| {
                let session = Arc::clone(&session);
                async move {
                    let mut session = session.lock().await;
                    session.handle_message(msg, tx).await
                }
            },
            pending_pack_tools,
            move |welcome_info, grpc_tx| {
                // Configure pack tools based on backend type
                let tool_count = welcome_info.available_tools.len();

                if let Some(ref cli) = cli_backend {
                    // Direct-CLI backend: Set MCP endpoint for Claude CLI to connect to gateway
                    if let Some(mcp_url) = welcome_info.mcp_url() {
                        tracing::info!(
                            mcp_url = %mcp_url,
                            tool_count = tool_count,
                            "Setting MCP endpoint for direct-cli backend"
                        );
                        cli.set_mcp_endpoint(mcp_url);
                    } else {
                        tracing::warn!("No MCP endpoint available - pack tools will not work");
                    }
                } else if let Some(ref mux) = mux_backend {
                    // Mux backend: Register PackTool instances for gRPC-routed execution
                    if tool_count > 0 {
                        if let Some(ref pending) = pending_for_callback {
                            tracing::info!(
                                tool_count = tool_count,
                                "Registering pack tools for mux backend"
                            );

                            // We need to spawn this because register_tool is async
                            // but on_welcome is a sync callback
                            let mux = mux.clone();
                            let pending = pending.clone();
                            let tools = welcome_info.available_tools.clone();
                            tokio::spawn(async move {
                                for tool_def in &tools {
                                    let pack_tool =
                                        PackTool::new(tool_def, grpc_tx.clone(), pending.clone());
                                    tracing::debug!(tool = %tool_def.name, "Registering pack tool");
                                    mux.register_tool(pack_tool).await;
                                }
                            });
                        }
                    }
                }
            },
        )
        .await?;

    Ok(())
}
