// ABOUTME: Entry point for coven-chat TUI
// ABOUTME: Handles CLI args, config loading, terminal setup, and async event loop

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

use coven_link::config::CovenConfig;
use coven_tui_v2::app::{Action, App};
use coven_tui_v2::client::{Client, Response, StateChange};
use coven_tui_v2::ui;

/// Terminal chat interface for coven agents
#[derive(Parser)]
#[command(name = "coven-chat")]
#[command(about = "Terminal chat interface for coven agents")]
struct Args {
    /// Agent to start chatting with (skips picker)
    #[arg(short, long)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Send a message non-interactively
    Send {
        /// The message to send
        message: String,
        /// Print the response to stdout
        #[arg(short, long)]
        print: bool,
    },
}

/// Get the TUI state directory (for persisted state like last agent, input history)
fn state_dir() -> Result<PathBuf> {
    let dir = CovenConfig::config_dir()?.join("tui");
    Ok(dir)
}

/// Get the gateway URL from coven config
fn gateway_url() -> Result<String> {
    let config = CovenConfig::load()
        .context("No coven config found. Run 'coven link' first to set up gateway connection.")?;

    // Normalize gateway address to URL
    let gateway = &config.gateway;
    if gateway.starts_with("http://") || gateway.starts_with("https://") {
        Ok(gateway.clone())
    } else {
        Ok(format!("http://{}", gateway))
    }
}

/// Get the SSH key path for authentication (use coven's device key)
fn ssh_key_path() -> Result<PathBuf> {
    CovenConfig::key_path()
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands (these don't need the TUI)
    match args.command {
        Some(Command::Send { message, print: _ }) => {
            let gw_url = gateway_url()?;
            let key_path = ssh_key_path()?;
            coven_tui_v2::cli::send::run(&gw_url, &key_path, &message, args.agent.as_deref())?;
            return Ok(());
        }
        None => {
            // Run interactive TUI
        }
    }

    // Get gateway URL from shared coven config
    let gw_url = gateway_url()?;

    // Create client BEFORE entering tokio runtime
    // (CovenClient creates its own runtime internally for FFI support)
    let key_path = ssh_key_path()?;
    let client = Client::new(&gw_url, &key_path)?;

    // Now run the async application with the pre-created client
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(run_app(args.agent, client))
}

/// Run the TUI application
async fn run_app(initial_agent: Option<String>, client: Client) -> Result<()> {
    // Set up terminal
    let mut terminal = setup_terminal()?;

    // Install panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal_basic();
        original_hook(panic_info);
    }));

    // Run the main loop, capturing the result
    let result = run_main_loop(&mut terminal, initial_agent, client).await;

    // Restore terminal (always, even on error)
    restore_terminal(&mut terminal)?;

    result
}

/// The main event loop
async fn run_main_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    initial_agent: Option<String>,
    client: Client,
) -> Result<()> {
    // Create channels
    let (response_tx, mut response_rx) = mpsc::channel::<Response>(32);
    let (state_tx, mut state_rx) = mpsc::channel::<StateChange>(32);
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(32);

    // Set up callbacks
    client.setup_callbacks(response_tx, state_tx);

    // Create app with persisted state
    let state_dir = state_dir()?;
    let mut app = App::load(&state_dir, initial_agent);

    // Initial connection status (use async version since we're in async context)
    app.connected = client.check_health_async().await.is_ok();

    // Fetch initial agent list
    match client.list_agents().await {
        Ok(agents) => {
            app.agents = agents;
        }
        Err(e) => {
            app.error = Some(format!("Failed to load agents: {}", e));
        }
    }

    // Load history for restored agent (if any)
    if let Some(agent_id) = &app.selected_agent {
        match client.load_history(agent_id).await {
            Ok(messages) => {
                app.messages = messages
                    .into_iter()
                    .map(coven_tui_v2::types::Message::from)
                    .collect();
            }
            Err(e) => {
                tracing::warn!("Failed to load history for {}: {}", agent_id, e);
            }
        }
    }

    // Spawn input task
    let _input_handle = spawn_input_task(key_tx);

    // Tick interval for throbber animation
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));

    // Main loop
    loop {
        // Render
        terminal.draw(|f| ui::render(f, &app))?;

        // Handle events with select!
        tokio::select! {
            // Key events from input task
            Some(key) = key_rx.recv() => {
                if let Some(action) = app.handle_key(key) {
                    match action {
                        Action::Quit => {
                            // Save state before quitting
                            if let Err(e) = app.save(&state_dir) {
                                tracing::warn!("Failed to save state: {}", e);
                            }
                            break;
                        }
                        Action::SendMessage(content) => {
                            if let Some(agent_id) = &app.selected_agent {
                                if let Err(e) = client.send_message(agent_id, &content) {
                                    app.error = Some(format!("Failed to send: {}", e));
                                    app.streaming = None;
                                    app.mode = coven_tui_v2::types::Mode::Chat;
                                }
                            }
                        }
                        Action::LoadHistory(agent_id) => {
                            match client.load_history(&agent_id).await {
                                Ok(messages) => {
                                    app.messages = messages
                                        .into_iter()
                                        .map(coven_tui_v2::types::Message::from)
                                        .collect();
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to load history: {}", e);
                                    // Not a fatal error - just start with empty chat
                                }
                            }
                        }
                        Action::RefreshAgents => {
                            match client.list_agents().await {
                                Ok(agents) => {
                                    app.agents = agents;
                                }
                                Err(e) => {
                                    app.error = Some(format!("Failed to refresh: {}", e));
                                }
                            }
                        }
                        Action::ApproveSelected => {
                            if let Some(approval) = app.get_selected_approval().cloned() {
                                match client
                                    .approve_tool_async(
                                        &approval.agent_id,
                                        &approval.tool_id,
                                        true,
                                        false,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        app.remove_approval(&approval.tool_id);
                                    }
                                    Err(e) => {
                                        app.error = Some(format!("Failed to approve: {}", e));
                                    }
                                }
                            }
                        }
                        Action::DenySelected => {
                            if let Some(approval) = app.get_selected_approval().cloned() {
                                match client
                                    .approve_tool_async(
                                        &approval.agent_id,
                                        &approval.tool_id,
                                        false,
                                        false,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        app.remove_approval(&approval.tool_id);
                                    }
                                    Err(e) => {
                                        app.error = Some(format!("Failed to deny: {}", e));
                                    }
                                }
                            }
                        }
                        Action::ApproveAllSelected => {
                            if let Some(approval) = app.get_selected_approval().cloned() {
                                match client
                                    .approve_tool_async(
                                        &approval.agent_id,
                                        &approval.tool_id,
                                        true,
                                        true,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        app.remove_approval(&approval.tool_id);
                                    }
                                    Err(e) => {
                                        app.error = Some(format!("Failed to approve all: {}", e));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Response events from client
            Some(response) = response_rx.recv() => {
                app.handle_response(response);
            }

            // State change events from client
            Some(state_change) = state_rx.recv() => {
                match state_change {
                    StateChange::ConnectionStatus(connected) => {
                        app.connected = connected;
                    }
                    StateChange::StreamingChanged(_agent_id, _is_streaming) => {
                        // Streaming state is handled by Response events
                    }
                    StateChange::MessagesChanged(_agent_id) => {
                        // Messages changed externally - could refresh here
                    }
                }
            }

            // Tick for throbber animation
            _ = tick_interval.tick() => {
                app.tick();
            }
        }
    }

    Ok(())
}

/// Spawn a blocking task to read crossterm key events
fn spawn_input_task(key_tx: mpsc::Sender<KeyEvent>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        loop {
            // Poll with a short timeout to check if channel is still open
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key_tx.blocking_send(key).is_err() {
                        break;
                    }
                }
            }

            // Check if channel is closed
            if key_tx.is_closed() {
                break;
            }
        }
    })
}

/// Set up the terminal for TUI rendering
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("Failed to create terminal")?;
    Ok(terminal)
}

/// Restore terminal to normal state
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;
    Ok(())
}

/// Basic terminal restoration for panic handler
fn restore_terminal_basic() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}
