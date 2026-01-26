// ABOUTME: Interactive TUI wizard for creating agent configurations
// ABOUTME: Guides user through name, backend, and server selection

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io;
use std::path::PathBuf;

/// Get XDG-style config directory (~/.config/coven)
/// Respects XDG_CONFIG_HOME if set, otherwise uses ~/.config
fn xdg_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|p| p.join("coven"))
}

/// Terminal guard for safe cleanup on panic or exit
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WizardStep {
    Name,
    Backend,
    Server,
    Review,
    SetDefault,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Backend {
    Mux,
    Cli,
}

impl Backend {
    fn as_str(&self) -> &'static str {
        match self {
            Backend::Mux => "mux",
            Backend::Cli => "cli",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Backend::Mux => "Direct Anthropic API (requires ANTHROPIC_API_KEY)",
            Backend::Cli => "Claude CLI subprocess (requires 'claude' binary)",
        }
    }
}

struct WizardApp {
    step: WizardStep,
    name: String,
    backend: Backend,
    server_host: String,
    server_port: String,
    server_field_focus: usize, // 0 = host, 1 = port
    error_message: Option<String>,
    should_quit: bool,
    saved_path: Option<String>,
    set_as_default: bool,
}

impl WizardApp {
    fn new() -> Self {
        Self {
            step: WizardStep::Name,
            name: String::new(),
            backend: Backend::Cli,
            server_host: "127.0.0.1".to_string(),
            server_port: "50051".to_string(),
            server_field_focus: 0,
            error_message: None,
            should_quit: false,
            saved_path: None,
            set_as_default: true,
        }
    }

    /// Combine host and port into a server URL
    fn server_url(&self) -> String {
        format!("http://{}:{}", self.server_host, self.server_port)
    }

    fn step_number(&self) -> usize {
        match self.step {
            WizardStep::Name => 1,
            WizardStep::Backend => 2,
            WizardStep::Server => 3,
            WizardStep::Review => 4,
            WizardStep::SetDefault => 5,
        }
    }

    fn total_steps(&self) -> usize {
        5
    }

    fn validate_name(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        // Check for invalid filename characters
        if self
            .name
            .contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        {
            return Err("Name contains invalid characters".to_string());
        }
        Ok(())
    }

    fn validate_server(&self) -> Result<(), String> {
        if self.server_host.trim().is_empty() {
            return Err("Host cannot be empty".to_string());
        }
        if self.server_port.trim().is_empty() {
            return Err("Port cannot be empty".to_string());
        }
        // Validate port is a valid number (1-65535)
        match self.server_port.parse::<u16>() {
            Ok(0) => return Err("Port cannot be 0".to_string()),
            Ok(_) => {}
            Err(_) => return Err("Port must be a valid number (1-65535)".to_string()),
        }
        Ok(())
    }

    fn next_step(&mut self) {
        self.error_message = None;

        match self.step {
            WizardStep::Name => {
                if let Err(e) = self.validate_name() {
                    self.error_message = Some(e);
                    return;
                }
                self.step = WizardStep::Backend;
            }
            WizardStep::Backend => {
                self.step = WizardStep::Server;
            }
            WizardStep::Server => {
                if let Err(e) = self.validate_server() {
                    self.error_message = Some(e);
                    return;
                }
                self.step = WizardStep::Review;
            }
            WizardStep::Review => {
                // Move to SetDefault step
                self.step = WizardStep::SetDefault;
            }
            WizardStep::SetDefault => {
                // Save config and optionally set as default
                if let Err(e) = self.save_config() {
                    self.error_message = Some(e.to_string());
                    return;
                }
                if self.set_as_default {
                    if let Err(e) = self.set_default() {
                        self.error_message = Some(e.to_string());
                        return;
                    }
                }
                self.should_quit = true;
            }
        }
    }

    fn prev_step(&mut self) {
        self.error_message = None;
        match self.step {
            WizardStep::Name => {
                self.should_quit = true;
            }
            WizardStep::Backend => {
                self.step = WizardStep::Name;
            }
            WizardStep::Server => {
                self.step = WizardStep::Backend;
            }
            WizardStep::Review => {
                self.step = WizardStep::Server;
            }
            WizardStep::SetDefault => {
                self.step = WizardStep::Review;
            }
        }
    }

    fn save_config(&mut self) -> Result<()> {
        let config_dir = xdg_config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("agents");

        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join(format!("{}.toml", self.name.trim()));
        let date = chrono::Local::now().format("%Y-%m-%d");

        let config_content = format!(
            r#"# Agent: {}
# Created: {}

name = "{}"
server = "{}"
backend = "{}"

# Working directory: uses current directory when agent is launched
# Uncomment and set to a specific path if needed:
# working_dir = "/path/to/project"
"#,
            self.name.trim(),
            date,
            self.name.trim(),
            self.server_url(),
            self.backend.as_str()
        );

        std::fs::write(&config_path, &config_content)?;

        // Also save project-local config (.coven/agent.toml) for auto-detection
        if let Ok(cwd) = std::env::current_dir() {
            let local_dir = cwd.join(".coven");
            let _ = std::fs::create_dir_all(&local_dir);
            let local_path = local_dir.join("agent.toml");
            let _ = std::fs::write(&local_path, &config_content);
        }

        self.saved_path = Some(config_path.display().to_string());

        Ok(())
    }

    fn set_default(&self) -> Result<()> {
        let config_dir =
            xdg_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

        std::fs::create_dir_all(&config_dir)?;

        let default_path = config_dir.join("agent.toml");
        let agent_path = config_dir
            .join("agents")
            .join(format!("{}.toml", self.name.trim()));

        // Copy the agent config to the default location
        std::fs::copy(&agent_path, &default_path)?;

        Ok(())
    }
}

pub async fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = WizardApp::new();

    // Main loop
    loop {
        terminal.draw(|f| draw_ui(f, &app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                handle_key_input(key.code, &mut app);
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Show success message after exiting TUI
    drop(_guard);
    if let Some(path) = app.saved_path {
        println!("\n✓ Agent configuration saved to: {}", path);
        if app.set_as_default {
            if let Some(default_path) = xdg_config_dir().map(|p| p.join("agent.toml")) {
                println!("✓ Set as default: {}", default_path.display());
                println!("\nRun your agent with:");
                println!("  coven-agent");
            }
        } else {
            println!("\nRun your agent with:");
            println!("  coven-agent --config {}", path);
        }
    }

    Ok(())
}

fn handle_key_input(key: KeyCode, app: &mut WizardApp) {
    match app.step {
        WizardStep::Name => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Char(c) => {
                app.name.push(c);
                app.error_message = None;
            }
            KeyCode::Backspace => {
                app.name.pop();
                app.error_message = None;
            }
            _ => {}
        },
        WizardStep::Backend => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Up | KeyCode::Char('k') => {
                app.backend = Backend::Mux;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.backend = Backend::Cli;
            }
            KeyCode::Char('1') => {
                app.backend = Backend::Mux;
            }
            KeyCode::Char('2') => {
                app.backend = Backend::Cli;
            }
            _ => {}
        },
        WizardStep::Server => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                // Toggle between host and port fields
                app.server_field_focus = 1 - app.server_field_focus;
            }
            KeyCode::Char(c) => {
                if app.server_field_focus == 0 {
                    app.server_host.push(c);
                } else {
                    // Only allow digits for port
                    if c.is_ascii_digit() {
                        app.server_port.push(c);
                    }
                }
                app.error_message = None;
            }
            KeyCode::Backspace => {
                if app.server_field_focus == 0 {
                    app.server_host.pop();
                } else {
                    app.server_port.pop();
                }
                app.error_message = None;
            }
            _ => {}
        },
        WizardStep::Review => match key {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app.next_step(),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app.prev_step(),
            _ => {}
        },
        WizardStep::SetDefault => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                app.set_as_default = !app.set_as_default;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.set_as_default = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                app.set_as_default = false;
            }
            _ => {}
        },
    }
}

fn draw_ui(f: &mut Frame, app: &WizardApp) {
    let size = f.area();

    // Create centered box
    let popup_area = centered_rect(60, 70, size);

    // Clear background
    f.render_widget(Clear, popup_area);

    // Main block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Create New Agent ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(block, popup_area);

    // Inner area
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(2), // Step indicator
            Constraint::Length(1), // Spacer
            Constraint::Min(8),    // Content
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Error message
            Constraint::Length(2), // Controls
        ])
        .split(popup_area);

    // Step indicator
    let step_text = format!(
        "Step {} of {}: {}",
        app.step_number(),
        app.total_steps(),
        match app.step {
            WizardStep::Name => "Agent Name",
            WizardStep::Backend => "Backend Selection",
            WizardStep::Server => "Server Address",
            WizardStep::Review => "Review Configuration",
            WizardStep::SetDefault => "Set as Default",
        }
    );
    let step_para = Paragraph::new(step_text).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(step_para, inner[0]);

    // Content area - depends on step
    match app.step {
        WizardStep::Name => draw_name_step(f, app, inner[2]),
        WizardStep::Backend => draw_backend_step(f, app, inner[2]),
        WizardStep::Server => draw_server_step(f, app, inner[2]),
        WizardStep::Review => draw_review_step(f, app, inner[2]),
        WizardStep::SetDefault => draw_set_default_step(f, app, inner[2]),
    }

    // Error message
    if let Some(ref error) = app.error_message {
        let error_para =
            Paragraph::new(format!("⚠ {}", error)).style(Style::default().fg(Color::Red));
        f.render_widget(error_para, inner[4]);
    }

    // Controls
    let controls = match app.step {
        WizardStep::Review => "[Enter/Y] Continue  [Esc/N] Go Back",
        WizardStep::SetDefault => "[Enter] Save  [↑/↓] Toggle  [Y/N] Yes/No  [Esc] Back",
        _ => "[Enter] Next  [Esc] Back/Cancel",
    };
    let controls_para = Paragraph::new(controls).style(Style::default().fg(Color::DarkGray));
    f.render_widget(controls_para, inner[5]);
}

fn draw_name_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(3), // Input
            Constraint::Min(1),    // Help
        ])
        .split(area);

    let label = Paragraph::new("Enter a unique name for your agent:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    let input_block =
        Block::default()
            .borders(Borders::ALL)
            .border_style(if app.error_message.is_some() {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            });

    let input = Paragraph::new(format!("{}_", app.name))
        .style(Style::default().fg(Color::White))
        .block(input_block);
    f.render_widget(input, chunks[1]);

    let help = Paragraph::new("This will be used as the config filename and agent identifier.")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[2]);
}

fn draw_backend_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Option 1
            Constraint::Length(2), // Option 2
            Constraint::Min(1),    // Help
        ])
        .split(area);

    let label =
        Paragraph::new("Select the backend to use:").style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    // Mux option
    let mux_style = if app.backend == Backend::Mux {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let mux_marker = if app.backend == Backend::Mux {
        "●"
    } else {
        "○"
    };
    let mux_text = Line::from(vec![
        Span::styled(format!(" {} ", mux_marker), mux_style),
        Span::styled("mux", mux_style),
        Span::styled(" - ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            Backend::Mux.description(),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(mux_text), chunks[2]);

    // Cli option
    let cli_style = if app.backend == Backend::Cli {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let cli_marker = if app.backend == Backend::Cli {
        "●"
    } else {
        "○"
    };
    let cli_text = Line::from(vec![
        Span::styled(format!(" {} ", cli_marker), cli_style),
        Span::styled("cli", cli_style),
        Span::styled(" - ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            Backend::Cli.description(),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(cli_text), chunks[3]);

    let help = Paragraph::new("[↑/↓] or [j/k] to select, [1/2] for quick select")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[4]);
}

fn draw_server_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Host input
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Port input
            Constraint::Min(1),    // Help
        ])
        .split(area);

    let label = Paragraph::new("Enter the gateway server address:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    // Host input
    let host_focused = app.server_field_focus == 0;
    let host_border_color = if app.error_message.is_some() {
        Color::Red
    } else if host_focused {
        Color::Green
    } else {
        Color::DarkGray
    };
    let host_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(host_border_color))
        .title(Span::styled(
            " Host ",
            Style::default().fg(if host_focused {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ));

    let host_text = if host_focused {
        format!("{}_", app.server_host)
    } else {
        app.server_host.clone()
    };
    let host_input = Paragraph::new(host_text)
        .style(Style::default().fg(Color::White))
        .block(host_block);
    f.render_widget(host_input, chunks[2]);

    // Port input
    let port_focused = app.server_field_focus == 1;
    let port_border_color = if app.error_message.is_some() {
        Color::Red
    } else if port_focused {
        Color::Green
    } else {
        Color::DarkGray
    };
    let port_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(port_border_color))
        .title(Span::styled(
            " Port ",
            Style::default().fg(if port_focused {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ));

    let port_text = if port_focused {
        format!("{}_", app.server_port)
    } else {
        app.server_port.clone()
    };
    let port_input = Paragraph::new(port_text)
        .style(Style::default().fg(Color::White))
        .block(port_block);
    f.render_widget(port_input, chunks[4]);

    let help = Paragraph::new("[Tab/↑/↓] Switch field  •  gRPC gateway your agent will connect to")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[5]);
}

fn draw_review_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Name
            Constraint::Length(1), // Backend
            Constraint::Length(1), // Server
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Path preview
        ])
        .split(area);

    let label = Paragraph::new("Review your configuration:").style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(label, chunks[0]);

    let name_line = Line::from(vec![
        Span::styled("  Name:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.name, Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(name_line), chunks[2]);

    let backend_line = Line::from(vec![
        Span::styled("  Backend: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.backend.as_str(), Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(backend_line), chunks[3]);

    let server_line = Line::from(vec![
        Span::styled("  Server:  ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.server_url(), Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(server_line), chunks[4]);

    let config_path = xdg_config_dir()
        .map(|p| p.join("agents").join(format!("{}.toml", app.name.trim())))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.config/coven/agents/<name>.toml".to_string());

    let path_line = Line::from(vec![
        Span::styled("  Will save to: ", Style::default().fg(Color::DarkGray)),
        Span::styled(config_path, Style::default().fg(Color::Green)),
    ]);
    f.render_widget(Paragraph::new(path_line), chunks[6]);
}

fn draw_set_default_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Label
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Yes option
            Constraint::Length(2), // No option
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Help
        ])
        .split(area);

    let label = Paragraph::new("Set this agent as your default?\nRunning 'coven-agent' without --config will use this agent.")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    // Yes option
    let yes_style = if app.set_as_default {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let yes_marker = if app.set_as_default { "●" } else { "○" };
    let yes_text = Line::from(vec![
        Span::styled(format!(" {} ", yes_marker), yes_style),
        Span::styled("Yes", yes_style),
        Span::styled(
            " - Set as default agent",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(yes_text), chunks[2]);

    // No option
    let no_style = if !app.set_as_default {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let no_marker = if !app.set_as_default { "●" } else { "○" };
    let no_text = Line::from(vec![
        Span::styled(format!(" {} ", no_marker), no_style),
        Span::styled("No", no_style),
        Span::styled(
            " - Only save to agents directory",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(no_text), chunks[3]);

    let default_path = xdg_config_dir()
        .map(|p| p.join("agent.toml").display().to_string())
        .unwrap_or_else(|| "~/.config/coven/agent.toml".to_string());

    let help = Paragraph::new(format!("Default config location: {}", default_path))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[5]);
}

/// Helper to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
