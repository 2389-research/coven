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
use serde::{Deserialize, Serialize};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

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
    /// First-time setup wizard
    Setup,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub gateway_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gateway_url: "http://localhost:7777".to_string(),
        }
    }
}

/// Get the configuration directory path
fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("coven-chat");
    Ok(dir)
}

/// Load configuration, creating default if missing
fn load_config() -> Result<Config> {
    let dir = config_dir()?;
    let config_path = dir.join("config.toml");

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .context("Failed to read config file")?;
        toml::from_str(&content).context("Failed to parse config file")
    } else {
        // Create default config
        std::fs::create_dir_all(&dir).context("Failed to create config directory")?;
        let config = Config::default();
        let content = toml::to_string_pretty(&config).context("Failed to serialize config")?;
        std::fs::write(&config_path, content).context("Failed to write default config")?;
        Ok(config)
    }
}

/// Get the SSH key path for authentication
fn ssh_key_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".ssh").join("id_ed25519"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands
    match args.command {
        Some(Command::Send { message, print }) => {
            // Non-interactive mode - not yet implemented
            if print {
                eprintln!("Send command not yet implemented: {}", message);
            }
            return Ok(());
        }
        Some(Command::Setup) => {
            eprintln!("Setup wizard not yet implemented");
            return Ok(());
        }
        None => {
            // Run interactive TUI
        }
    }

    // Load config
    let config = load_config()?;

    // Run the application
    run_app(config, args.agent).await
}

/// Run the TUI application
async fn run_app(config: Config, initial_agent: Option<String>) -> Result<()> {
    // Set up terminal
    let mut terminal = setup_terminal()?;

    // Install panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal_basic();
        original_hook(panic_info);
    }));

    // Run the main loop, capturing the result
    let result = run_main_loop(&mut terminal, config, initial_agent).await;

    // Restore terminal (always, even on error)
    restore_terminal(&mut terminal)?;

    result
}

/// The main event loop
async fn run_main_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    config: Config,
    initial_agent: Option<String>,
) -> Result<()> {
    // Create channels
    let (response_tx, mut response_rx) = mpsc::channel::<Response>(32);
    let (state_tx, mut state_rx) = mpsc::channel::<StateChange>(32);
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(32);

    // Create client
    let ssh_key = ssh_key_path()?;
    let client = Client::new(&config.gateway_url, &ssh_key)?;

    // Set up callbacks
    client.setup_callbacks(response_tx, state_tx);

    // Create app with persisted state
    let config_dir = config_dir()?;
    let mut app = App::load(&config_dir, initial_agent);

    // Initial connection status
    app.connected = client.check_health().is_ok();

    // Fetch initial agent list
    match client.list_agents().await {
        Ok(agents) => {
            app.agents = agents;
        }
        Err(e) => {
            app.error = Some(format!("Failed to load agents: {}", e));
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
                            if let Err(e) = app.save(&config_dir) {
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

/// Spawn a blocking task to read crossterm events
fn spawn_input_task(key_tx: mpsc::Sender<KeyEvent>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        loop {
            // Poll with a short timeout to check if channel is still open
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    // Try to send, exit if channel closed
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
