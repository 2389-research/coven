// ABOUTME: Interactive TUI wizard for creating agent configurations
// ABOUTME: Guides user through name, backend, server, capabilities, and scope selection

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
    Capabilities,
    Scope,
    Review,
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

/// Available capabilities that can be selected
#[derive(Debug, Clone, Copy, PartialEq)]
enum Capability {
    Base,
    Chat,
    Llm,
    Filesystem,
    Admin,
}

impl Capability {
    fn as_str(&self) -> &'static str {
        match self {
            Capability::Base => "base",
            Capability::Chat => "chat",
            Capability::Llm => "llm",
            Capability::Filesystem => "filesystem",
            Capability::Admin => "admin",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Capability::Base => "Access to gateway builtin tools (log, todo, bbs)",
            Capability::Chat => "Basic chat/messaging capability",
            Capability::Llm => "LLM inference capabilities",
            Capability::Filesystem => "Filesystem access tools",
            Capability::Admin => "Administrative tools (agent management)",
        }
    }

    fn all() -> &'static [Capability] {
        &[
            Capability::Base,
            Capability::Chat,
            Capability::Llm,
            Capability::Filesystem,
            Capability::Admin,
        ]
    }
}

/// Where to save the configuration
#[derive(Debug, Clone, Copy, PartialEq)]
enum ConfigScope {
    /// Project-local: .coven/agent.toml
    Project,
    /// User library: ~/.config/coven/agents/{name}.toml
    UserLibrary,
    /// User default: ~/.config/coven/agent.toml
    UserDefault,
}

impl ConfigScope {
    fn description(&self) -> &'static str {
        match self {
            ConfigScope::Project => "Project only (.coven/agent.toml)",
            ConfigScope::UserLibrary => "User library (~/.config/coven/agents/{name}.toml)",
            ConfigScope::UserDefault => "User default (~/.config/coven/agent.toml)",
        }
    }

    fn path_preview(&self, name: &str) -> String {
        match self {
            ConfigScope::Project => ".coven/agent.toml".to_string(),
            ConfigScope::UserLibrary => {
                xdg_config_dir()
                    .map(|p| p.join("agents").join(format!("{}.toml", name.trim())))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| format!("~/.config/coven/agents/{}.toml", name.trim()))
            }
            ConfigScope::UserDefault => {
                xdg_config_dir()
                    .map(|p| p.join("agent.toml"))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "~/.config/coven/agent.toml".to_string())
            }
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
    capabilities: Vec<bool>,   // Parallel to Capability::all()
    capability_focus: usize,
    scope: ConfigScope,
    also_set_default: bool, // Only used when scope is UserLibrary
    error_message: Option<String>,
    should_quit: bool,
    saved_paths: Vec<String>,
}

impl WizardApp {
    fn new() -> Self {
        // Try to load coven config (created by `coven link`) for prefilling gateway
        let coven_config = coven_link::config::CovenConfig::load().ok();

        // Parse gateway address into host and port if available
        let (server_host, server_port) = coven_config
            .as_ref()
            .map(|c| {
                let gateway = c
                    .gateway
                    .strip_prefix("http://")
                    .or_else(|| c.gateway.strip_prefix("https://"))
                    .unwrap_or(&c.gateway);

                if let Some((host, port)) = gateway.rsplit_once(':') {
                    (host.to_string(), port.to_string())
                } else {
                    (gateway.to_string(), "50051".to_string())
                }
            })
            .unwrap_or_else(|| ("127.0.0.1".to_string(), "50051".to_string()));

        // Get device name from config as suggested agent name
        let name = coven_config
            .as_ref()
            .map(|c| c.device_name.clone())
            .unwrap_or_default();

        // Default capabilities: base and chat enabled (matching default_capabilities())
        let default_caps = crate::metadata::default_capabilities();
        let mut capabilities = vec![false; Capability::all().len()];
        for (idx, cap) in Capability::all().iter().enumerate() {
            capabilities[idx] = default_caps.contains(&cap.as_str().to_string());
        }

        Self {
            step: WizardStep::Name,
            name,
            backend: Backend::Cli,
            server_host,
            server_port,
            server_field_focus: 0,
            capabilities,
            capability_focus: 0,
            scope: ConfigScope::UserLibrary,
            also_set_default: false,
            error_message: None,
            should_quit: false,
            saved_paths: Vec::new(),
        }
    }

    fn server_url(&self) -> String {
        format!("http://{}:{}", self.server_host, self.server_port)
    }

    fn selected_capabilities(&self) -> Vec<&'static str> {
        Capability::all()
            .iter()
            .zip(self.capabilities.iter())
            .filter_map(|(cap, &selected)| if selected { Some(cap.as_str()) } else { None })
            .collect()
    }

    fn capabilities_toml(&self) -> String {
        let caps: Vec<_> = self.selected_capabilities();
        format!(
            "[{}]",
            caps.iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn step_number(&self) -> usize {
        match self.step {
            WizardStep::Name => 1,
            WizardStep::Backend => 2,
            WizardStep::Server => 3,
            WizardStep::Capabilities => 4,
            WizardStep::Scope => 5,
            WizardStep::Review => 6,
        }
    }

    fn total_steps(&self) -> usize {
        6
    }

    fn validate_name(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name cannot be empty".to_string());
        }
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
        match self.server_port.parse::<u16>() {
            Ok(0) => return Err("Port cannot be 0".to_string()),
            Ok(_) => {}
            Err(_) => return Err("Port must be a valid number (1-65535)".to_string()),
        }
        Ok(())
    }

    fn validate_capabilities(&self) -> Result<(), String> {
        if !self.capabilities.iter().any(|&c| c) {
            return Err("At least one capability must be selected".to_string());
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
                self.step = WizardStep::Capabilities;
            }
            WizardStep::Capabilities => {
                if let Err(e) = self.validate_capabilities() {
                    self.error_message = Some(e);
                    return;
                }
                self.step = WizardStep::Scope;
            }
            WizardStep::Scope => {
                self.step = WizardStep::Review;
            }
            WizardStep::Review => {
                if let Err(e) = self.save_config() {
                    self.error_message = Some(e.to_string());
                    return;
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
            WizardStep::Capabilities => {
                self.step = WizardStep::Server;
            }
            WizardStep::Scope => {
                self.step = WizardStep::Capabilities;
            }
            WizardStep::Review => {
                self.step = WizardStep::Scope;
            }
        }
    }

    fn generate_config_content(&self) -> String {
        let date = chrono::Local::now().format("%Y-%m-%d");
        format!(
            r#"# Agent: {}
# Created: {}

name = "{}"
server = "{}"
backend = "{}"
capabilities = {}

# Working directory: uses current directory when agent is launched
# Uncomment and set to a specific path if needed:
# working_dir = "/path/to/project"
"#,
            self.name.trim(),
            date,
            self.name.trim(),
            self.server_url(),
            self.backend.as_str(),
            self.capabilities_toml()
        )
    }

    fn save_config(&mut self) -> Result<()> {
        let config_content = self.generate_config_content();
        self.saved_paths.clear();

        match self.scope {
            ConfigScope::Project => {
                let cwd = std::env::current_dir()?;
                let local_dir = cwd.join(".coven");
                std::fs::create_dir_all(&local_dir)?;
                let local_path = local_dir.join("agent.toml");
                std::fs::write(&local_path, &config_content)?;
                self.saved_paths.push(local_path.display().to_string());
            }
            ConfigScope::UserLibrary => {
                let config_dir = xdg_config_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
                    .join("agents");
                std::fs::create_dir_all(&config_dir)?;
                let config_path = config_dir.join(format!("{}.toml", self.name.trim()));
                std::fs::write(&config_path, &config_content)?;
                self.saved_paths.push(config_path.display().to_string());

                // Also set as default if requested
                if self.also_set_default {
                    let default_dir = xdg_config_dir()
                        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
                    std::fs::create_dir_all(&default_dir)?;
                    let default_path = default_dir.join("agent.toml");
                    std::fs::write(&default_path, &config_content)?;
                    self.saved_paths.push(default_path.display().to_string());
                }
            }
            ConfigScope::UserDefault => {
                let config_dir = xdg_config_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
                std::fs::create_dir_all(&config_dir)?;
                let config_path = config_dir.join("agent.toml");
                std::fs::write(&config_path, &config_content)?;
                self.saved_paths.push(config_path.display().to_string());
            }
        }

        Ok(())
    }
}

/// Run the wizard with the specified command prefix for output messages.
pub async fn run_with_prefix(command_prefix: &str) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = WizardApp::new();

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

    drop(_guard);

    if !app.saved_paths.is_empty() {
        println!();
        for path in &app.saved_paths {
            println!("  Saved: {}", path);
        }
        println!();

        // Show run instructions based on scope
        match app.scope {
            ConfigScope::Project => {
                println!("Run your agent from this directory with:");
                println!("  {} run", command_prefix);
            }
            ConfigScope::UserLibrary if app.also_set_default => {
                println!("Run your agent with:");
                println!("  {} run", command_prefix);
            }
            ConfigScope::UserLibrary => {
                println!("Run your agent with:");
                println!(
                    "  {} run --config {}",
                    command_prefix,
                    app.saved_paths.first().unwrap()
                );
            }
            ConfigScope::UserDefault => {
                println!("Run your agent with:");
                println!("  {} run", command_prefix);
            }
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
            KeyCode::Up | KeyCode::Char('k') => app.backend = Backend::Mux,
            KeyCode::Down | KeyCode::Char('j') => app.backend = Backend::Cli,
            KeyCode::Char('1') => app.backend = Backend::Mux,
            KeyCode::Char('2') => app.backend = Backend::Cli,
            _ => {}
        },
        WizardStep::Server => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                app.server_field_focus = 1 - app.server_field_focus;
            }
            KeyCode::Char(c) => {
                if app.server_field_focus == 0 {
                    app.server_host.push(c);
                } else if c.is_ascii_digit() {
                    app.server_port.push(c);
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
        WizardStep::Capabilities => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Up | KeyCode::Char('k') => {
                if app.capability_focus > 0 {
                    app.capability_focus -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.capability_focus < Capability::all().len() - 1 {
                    app.capability_focus += 1;
                }
            }
            KeyCode::Char(' ') => {
                app.capabilities[app.capability_focus] = !app.capabilities[app.capability_focus];
                app.error_message = None;
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let idx = c.to_digit(10).unwrap() as usize;
                if idx > 0 && idx <= Capability::all().len() {
                    app.capabilities[idx - 1] = !app.capabilities[idx - 1];
                    app.error_message = None;
                }
            }
            _ => {}
        },
        WizardStep::Scope => match key {
            KeyCode::Enter => app.next_step(),
            KeyCode::Esc => app.prev_step(),
            KeyCode::Up | KeyCode::Char('k') => {
                // Non-wrapping navigation - stop at top
                app.scope = match app.scope {
                    ConfigScope::Project => ConfigScope::Project,
                    ConfigScope::UserLibrary => ConfigScope::Project,
                    ConfigScope::UserDefault => ConfigScope::UserLibrary,
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Non-wrapping navigation - stop at bottom
                app.scope = match app.scope {
                    ConfigScope::Project => ConfigScope::UserLibrary,
                    ConfigScope::UserLibrary => ConfigScope::UserDefault,
                    ConfigScope::UserDefault => ConfigScope::UserDefault,
                };
            }
            KeyCode::Char('1') => app.scope = ConfigScope::Project,
            KeyCode::Char('2') => app.scope = ConfigScope::UserLibrary,
            KeyCode::Char('3') => app.scope = ConfigScope::UserDefault,
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if app.scope == ConfigScope::UserLibrary {
                    app.also_set_default = !app.also_set_default;
                }
            }
            _ => {}
        },
        WizardStep::Review => match key {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app.next_step(),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app.prev_step(),
            _ => {}
        },
    }
}

fn draw_ui(f: &mut Frame, app: &WizardApp) {
    let size = f.area();
    let popup_area = centered_rect(70, 80, size);

    f.render_widget(Clear, popup_area);

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

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(2), // Step indicator
            Constraint::Length(1), // Spacer
            Constraint::Min(10),   // Content
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Error message
            Constraint::Length(2), // Controls
        ])
        .split(popup_area);

    let step_text = format!(
        "Step {} of {}: {}",
        app.step_number(),
        app.total_steps(),
        match app.step {
            WizardStep::Name => "Agent Name",
            WizardStep::Backend => "Backend Selection",
            WizardStep::Server => "Server Address",
            WizardStep::Capabilities => "Capabilities",
            WizardStep::Scope => "Config Location",
            WizardStep::Review => "Review Configuration",
        }
    );
    let step_para = Paragraph::new(step_text).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(step_para, inner[0]);

    match app.step {
        WizardStep::Name => draw_name_step(f, app, inner[2]),
        WizardStep::Backend => draw_backend_step(f, app, inner[2]),
        WizardStep::Server => draw_server_step(f, app, inner[2]),
        WizardStep::Capabilities => draw_capabilities_step(f, app, inner[2]),
        WizardStep::Scope => draw_scope_step(f, app, inner[2]),
        WizardStep::Review => draw_review_step(f, app, inner[2]),
    }

    if let Some(ref error) = app.error_message {
        let error_para =
            Paragraph::new(format!(" {}", error)).style(Style::default().fg(Color::Red));
        f.render_widget(error_para, inner[4]);
    }

    let controls = match app.step {
        WizardStep::Capabilities => "[Space] Toggle  [1-5] Quick toggle  [Enter] Next  [Esc] Back",
        WizardStep::Scope if app.scope == ConfigScope::UserLibrary => {
            "[1-3] Select  [D] Toggle default  [Enter] Next  [Esc] Back"
        }
        WizardStep::Scope => "[1-3] Select  [Enter] Next  [Esc] Back",
        WizardStep::Review => "[Enter/Y] Save  [Esc/N] Go Back",
        _ => "[Enter] Next  [Esc] Back/Cancel",
    };
    let controls_para = Paragraph::new(controls).style(Style::default().fg(Color::DarkGray));
    f.render_widget(controls_para, inner[5]);
}

fn draw_name_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    let label = Paragraph::new("Enter a unique name for your agent:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    let input_block = Block::default()
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
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(area);

    let label =
        Paragraph::new("Select the backend to use:").style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    for (idx, (backend, desc)) in [
        (Backend::Mux, Backend::Mux.description()),
        (Backend::Cli, Backend::Cli.description()),
    ]
    .iter()
    .enumerate()
    {
        let is_selected = app.backend == *backend;
        let style = if is_selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if is_selected { "" } else { "" };
        let text = Line::from(vec![
            Span::styled(format!(" {} ", marker), style),
            Span::styled(backend.as_str(), style),
            Span::styled(" - ", Style::default().fg(Color::DarkGray)),
            Span::styled(*desc, Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(text), chunks[2 + idx]);
    }

    let help = Paragraph::new("[/] or [j/k] to select, [1/2] for quick select")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[4]);
}

fn draw_server_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    let label = Paragraph::new("Enter the gateway server address:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

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

    let help = Paragraph::new("[Tab//] Switch field")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[5]);
}

fn draw_capabilities_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let cap_count = Capability::all().len();
    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1)];
    for _ in 0..cap_count {
        constraints.push(Constraint::Length(2));
    }
    constraints.push(Constraint::Min(1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let label = Paragraph::new("Select capabilities for this agent:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    for (idx, cap) in Capability::all().iter().enumerate() {
        let is_selected = app.capabilities[idx];
        let is_focused = app.capability_focus == idx;

        let checkbox = if is_selected { "[x]" } else { "[ ]" };

        let style = if is_focused {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };

        let text = Line::from(vec![
            Span::styled(format!(" {} {} ", idx + 1, checkbox), style),
            Span::styled(cap.as_str(), style),
            Span::styled(" - ", Style::default().fg(Color::DarkGray)),
            Span::styled(cap.description(), Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(text), chunks[2 + idx]);
    }
}

fn draw_scope_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let label = Paragraph::new("Where should this config be saved?")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, chunks[0]);

    let scopes = [
        ConfigScope::Project,
        ConfigScope::UserLibrary,
        ConfigScope::UserDefault,
    ];

    for (idx, scope) in scopes.iter().enumerate() {
        let is_selected = app.scope == *scope;
        let style = if is_selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if is_selected { "" } else { "" };

        let mut spans = vec![
            Span::styled(format!(" {} {} ", idx + 1, marker), style),
            Span::styled(scope.description(), style),
        ];

        // Show "also set default" toggle for UserLibrary
        if *scope == ConfigScope::UserLibrary && is_selected {
            let default_text = if app.also_set_default {
                " [D: also default]"
            } else {
                ""
            };
            spans.push(Span::styled(
                default_text,
                Style::default().fg(Color::Yellow),
            ));
        }

        let text = Line::from(spans);
        f.render_widget(Paragraph::new(text), chunks[2 + idx]);
    }

    let path_preview = app.scope.path_preview(&app.name);
    let preview_line = Line::from(vec![
        Span::styled("  Will save to: ", Style::default().fg(Color::DarkGray)),
        Span::styled(path_preview, Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(preview_line), chunks[6]);
}

fn draw_review_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let label = Paragraph::new("Review your configuration:").style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(label, chunks[0]);

    let fields = [
        ("Name:", app.name.clone()),
        ("Backend:", app.backend.as_str().to_string()),
        ("Server:", app.server_url()),
        (
            "Capabilities:",
            app.selected_capabilities().join(", "),
        ),
        ("Location:", app.scope.path_preview(&app.name)),
    ];

    for (idx, (label, value)) in fields.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(format!("  {:14}", label), Style::default().fg(Color::DarkGray)),
            Span::styled(value, Style::default().fg(Color::Cyan)),
        ]);
        f.render_widget(Paragraph::new(line), chunks[2 + idx]);
    }

    if app.scope == ConfigScope::UserLibrary && app.also_set_default {
        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "(will also be set as user default)",
                Style::default().fg(Color::Yellow),
            ),
        ]);
        f.render_widget(Paragraph::new(line), chunks[7]);
    }
}

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
