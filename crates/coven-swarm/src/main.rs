// ABOUTME: coven-swarm CLI entry point.
// ABOUTME: Provides init, supervisor, and agent subcommands.

mod agent;
mod init;
mod supervisor;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "coven-swarm")]
#[command(about = "Supervisor for coven workspace agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new coven-swarm configuration
    Init,
    /// Run the supervisor daemon
    Supervisor {
        /// Path to configuration file
        #[arg(long)]
        config: Option<PathBuf>,
        /// Headless mode (minimal output, no TUI)
        #[arg(long)]
        headless: bool,
    },
    /// Run a single workspace agent (internal, spawned by supervisor)
    Agent {
        /// Workspace name
        #[arg(long)]
        workspace: String,
        /// Run in dispatch mode (swarm management tools)
        #[arg(long)]
        dispatch_mode: bool,
        /// Path to configuration file
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (ignore errors if not found)
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => init::run_init(),
        Commands::Supervisor { config: config_path, headless } => {
            use crate::supervisor::{discover_workspaces, socket, AgentProcess, Tui, TuiEvent};
            use coven_swarm_core::Config;
            use std::collections::HashMap;
            use tokio::sync::mpsc;

            let config_path = config_path.unwrap_or_else(|| {
                Config::default_path().expect("Failed to get default config path")
            });
            let config = Config::load(&config_path)?;
            let working_dir = config.working_directory_expanded();

            // Create TUI if not headless
            let tui_tx: Option<mpsc::Sender<TuiEvent>> = if headless {
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
                let mut agent =
                    AgentProcess::new(workspace.clone(), config_path.clone(), dispatch_mode);
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
                let mut dispatch =
                    AgentProcess::new("dispatch".to_string(), config_path.clone(), true);
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
                    message: format!("Socket server listening"),
                });
            }

            // Handle socket commands
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    socket::SocketCommand::List { reply } => {
                        send_event(&tui_tx, TuiEvent::SocketCommand {
                            command: "list".to_string(),
                        });
                        let names: Vec<String> = agents.keys().cloned().collect();
                        let _ = reply.send(names);
                    }
                    socket::SocketCommand::Create { name, reply } => {
                        send_event(&tui_tx, TuiEvent::SocketCommand {
                            command: format!("create {}", name),
                        });
                        if agents.contains_key(&name) {
                            let _ = reply.send(Err(anyhow::anyhow!("Workspace already exists")));
                            continue;
                        }
                        std::fs::create_dir_all(working_dir.join(&name))?;
                        let mut agent =
                            AgentProcess::new(name.clone(), config_path.clone(), false);
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
                    socket::SocketCommand::Delete { name, reply } => {
                        send_event(&tui_tx, TuiEvent::SocketCommand {
                            command: format!("delete {}", name),
                        });
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
                    socket::SocketCommand::Status { reply } => {
                        send_event(&tui_tx, TuiEvent::SocketCommand {
                            command: "status".to_string(),
                        });
                        let agent_statuses: Vec<socket::AgentStatus> = agents
                            .iter()
                            .map(|(workspace_name, agent)| socket::AgentStatus {
                                workspace: workspace_name.clone(),
                                running: agent.is_running(),
                                pid: agent.pid(),
                            })
                            .collect();
                        let status_info = socket::StatusInfo {
                            prefix: config.prefix.clone(),
                            agents: agent_statuses,
                        };
                        let _ = reply.send(status_info);
                    }
                    socket::SocketCommand::Stop { reply } => {
                        send_event(&tui_tx, TuiEvent::SocketCommand {
                            command: "stop".to_string(),
                        });
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
        Commands::Agent {
            workspace,
            dispatch_mode,
            config: config_path,
        } => {
            use coven_swarm_core::Config;
            use coven_core::backend::{MuxBackend, MuxConfig};
            use coven_swarm_backend::BackendHandle;
            use coven_swarm_backend::dispatch_tools::{ListAgentsTool, CreateWorkspaceTool, DeleteWorkspaceTool};
            use crate::agent::{GatewayClient, Session};
            use std::sync::Arc;
            use tokio::sync::Mutex;

            // Load or generate SSH key
            let key_path = coven_ssh::default_swarm_key_path()
                .ok_or_else(|| anyhow::anyhow!("Could not determine SSH key path"))?;
            let private_key = coven_ssh::load_or_generate_key(&key_path)?;
            let fingerprint = coven_ssh::compute_fingerprint(private_key.public_key())?;
            tracing::info!(fingerprint = %fingerprint, "SSH key loaded");

            // Load config
            let config_path = config_path.unwrap_or_else(|| {
                Config::default_path().expect("Failed to get default config path")
            });
            let config = Config::load(&config_path)?;

            let working_dir = config.working_directory_expanded().join(&workspace);

            // Validate working directory exists
            if !working_dir.exists() {
                anyhow::bail!(
                    "Workspace directory does not exist: {}",
                    working_dir.display()
                );
            }

            // Create backend based on mode
            let (handle, backend_name) = if dispatch_mode {
                // Dispatch mode uses coven-core's MuxBackend with dispatch tools
                let mux_config = MuxConfig {
                    model: "claude-sonnet-4-20250514".to_string(),
                    max_tokens: 8192,
                    working_dir: working_dir.clone(),
                    global_system_prompt_path: None,
                    local_prompt_files: vec!["claude.md".to_string(), "CLAUDE.md".to_string()],
                    skip_default_tools: false,
                    ..MuxConfig::default()
                };
                let backend = MuxBackend::new(mux_config).await?;

                // Register dispatch tools
                backend.register_tool(ListAgentsTool::new(config.prefix.clone())).await;
                backend.register_tool(CreateWorkspaceTool::new(config.prefix.clone())).await;
                backend.register_tool(DeleteWorkspaceTool::new(config.prefix.clone())).await;
                tracing::info!("Registered dispatch tools: list_agents, create_workspace, delete_workspace");

                let handle = BackendHandle::new(backend);
                let name = handle.name();
                (handle, name)
            } else {
                // Normal workspace uses ACP backend
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
                    // Fallback to coven-core's MuxBackend when ACP is not available
                    let mux_config = MuxConfig {
                        model: "claude-sonnet-4-20250514".to_string(),
                        max_tokens: 8192,
                        working_dir: working_dir.clone(),
                        global_system_prompt_path: None,
                        local_prompt_files: vec!["claude.md".to_string(), "CLAUDE.md".to_string()],
                        skip_default_tools: false,
                        ..MuxConfig::default()
                    };
                    let backend = MuxBackend::new(mux_config).await?;
                    let handle = BackendHandle::new(backend);
                    let name = handle.name();
                    (handle, name)
                }
            };

            let session = Arc::new(Mutex::new(Session::new(handle)));

            let gateway_url = config.gateway_url()?;

            tracing::info!(
                workspace = %workspace,
                working_dir = %working_dir.display(),
                gateway = %gateway_url,
                backend = %backend_name,
                dispatch_mode = %dispatch_mode,
                "Starting agent"
            );

            // Connect to gateway
            let client = GatewayClient::connect(
                &gateway_url,
                &config.prefix,
                &workspace,
                &working_dir.to_string_lossy(),
                backend_name,
            ).await?;

            // Run the agent with real-time response streaming
            client.run(|msg, tx| {
                let session = Arc::clone(&session);
                async move {
                    let mut session = session.lock().await;
                    session.handle_message(msg, tx).await
                }
            }).await?;

            Ok(())
        }
    }
}
