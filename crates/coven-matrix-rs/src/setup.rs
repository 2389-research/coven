// ABOUTME: Interactive TUI setup wizard for configuring the Matrix bridge.
// ABOUTME: Form-based configuration matching the style of coven-tui and coven-agent wizards.

use crate::error::{BridgeError, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};
use std::fs;
use std::io;
use std::path::PathBuf;

/// Terminal guard for safe cleanup on panic or exit
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Get XDG-style config directory (~/.config/coven)
fn xdg_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|p| p.join("coven"))
}

/// Existing coven config from coven-link (if present).
#[derive(Debug, Default)]
struct ExistingConfig {
    gateway_host: Option<String>,
    gateway_port: Option<u16>,
    token: Option<String>,
}

impl ExistingConfig {
    /// Load from coven-link config (same as coven-agent wizard).
    fn load() -> Self {
        let coven_config = coven_link::config::CovenConfig::load().ok();

        // Parse gateway address into host and port if available
        let (gateway_host, gateway_port) = coven_config
            .as_ref()
            .map(|c| {
                let gateway = c
                    .gateway
                    .strip_prefix("http://")
                    .or_else(|| c.gateway.strip_prefix("https://"))
                    .unwrap_or(&c.gateway);

                if let Some((host, port_str)) = gateway.rsplit_once(':') {
                    (Some(host.to_string()), port_str.parse().ok())
                } else {
                    (Some(gateway.to_string()), None)
                }
            })
            .unwrap_or((None, None));

        // Get token from coven config
        let token = coven_config.as_ref().and_then(|c| {
            if c.token.is_empty() {
                None
            } else {
                Some(c.token.clone())
            }
        });

        Self {
            gateway_host,
            gateway_port,
            token,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WizardStep {
    Matrix,
    Gateway,
    AccessControl,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MatrixField {
    Homeserver,
    Username,
    Password,
    RecoveryKey,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GatewayField {
    Host,
    Port,
    Tls,
    Token,
}

struct WizardApp {
    step: WizardStep,
    // Matrix fields
    homeserver: String,
    username: String,
    password: String,
    use_env_password: bool,
    recovery_key: String,
    use_env_recovery_key: bool,
    matrix_focus: MatrixField,
    // Gateway fields
    gateway_host: String,
    gateway_port: String,
    gateway_tls: bool,
    gateway_token: String,
    use_env_token: bool,
    use_existing_token: bool,
    gateway_focus: GatewayField,
    // Access control
    restrict_rooms: bool,
    allowed_rooms: String,
    restrict_senders: bool,
    allowed_senders: String,
    use_prefix: bool,
    command_prefix: String,
    typing_indicator: bool,
    access_focus: usize,
    // State
    existing_token: Option<String>,
    error_message: Option<String>,
    should_quit: bool,
    saved_path: Option<String>,
}

impl WizardApp {
    fn new() -> Self {
        let existing = ExistingConfig::load();

        Self {
            step: WizardStep::Matrix,
            // Matrix
            homeserver: "https://matrix.org".to_string(),
            username: String::new(),
            password: String::new(),
            use_env_password: true,
            recovery_key: String::new(),
            use_env_recovery_key: true,
            matrix_focus: MatrixField::Homeserver,
            // Gateway - prefill from existing config
            gateway_host: existing
                .gateway_host
                .unwrap_or_else(|| "localhost".to_string()),
            gateway_port: existing
                .gateway_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "6666".to_string()),
            gateway_tls: false,
            gateway_token: String::new(),
            use_env_token: existing.token.is_none(),
            use_existing_token: existing.token.is_some(),
            gateway_focus: GatewayField::Host,
            // Access control
            restrict_rooms: false,
            allowed_rooms: String::new(),
            restrict_senders: false,
            allowed_senders: String::new(),
            use_prefix: false,
            command_prefix: "!coven ".to_string(),
            typing_indicator: true,
            access_focus: 0,
            // State
            existing_token: existing.token,
            error_message: None,
            should_quit: false,
            saved_path: None,
        }
    }

    fn step_number(&self) -> usize {
        match self.step {
            WizardStep::Matrix => 1,
            WizardStep::Gateway => 2,
            WizardStep::AccessControl => 3,
            WizardStep::Review => 4,
        }
    }

    fn validate_matrix(&self) -> std::result::Result<(), String> {
        if self.homeserver.trim().is_empty() {
            return Err("Homeserver URL is required".to_string());
        }
        if !self.homeserver.starts_with("http://") && !self.homeserver.starts_with("https://") {
            return Err("Homeserver must start with http:// or https://".to_string());
        }
        if self.username.trim().is_empty() {
            return Err("Username is required".to_string());
        }
        if !self.use_env_password && self.password.is_empty() {
            return Err("Password is required (or use env variable)".to_string());
        }
        Ok(())
    }

    fn validate_gateway(&self) -> std::result::Result<(), String> {
        if self.gateway_host.trim().is_empty() {
            return Err("Gateway host is required".to_string());
        }
        match self.gateway_port.parse::<u16>() {
            Ok(0) => return Err("Port cannot be 0".to_string()),
            Ok(_) => {}
            Err(_) => return Err("Port must be a valid number".to_string()),
        }
        Ok(())
    }

    fn next_step(&mut self) {
        self.error_message = None;
        match self.step {
            WizardStep::Matrix => {
                if let Err(e) = self.validate_matrix() {
                    self.error_message = Some(e);
                    return;
                }
                self.step = WizardStep::Gateway;
            }
            WizardStep::Gateway => {
                if let Err(e) = self.validate_gateway() {
                    self.error_message = Some(e);
                    return;
                }
                self.step = WizardStep::AccessControl;
            }
            WizardStep::AccessControl => {
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
            WizardStep::Matrix => self.should_quit = true,
            WizardStep::Gateway => self.step = WizardStep::Matrix,
            WizardStep::AccessControl => self.step = WizardStep::Gateway,
            WizardStep::Review => self.step = WizardStep::AccessControl,
        }
    }

    fn get_password_value(&self) -> String {
        if self.use_env_password {
            "${MATRIX_PASSWORD}".to_string()
        } else {
            self.password.clone()
        }
    }

    fn get_recovery_key_value(&self) -> Option<String> {
        if self.use_env_recovery_key {
            Some("${MATRIX_RECOVERY_KEY}".to_string())
        } else if self.recovery_key.is_empty() {
            None
        } else {
            Some(self.recovery_key.clone())
        }
    }

    fn get_token_value(&self) -> Option<String> {
        if self.use_existing_token {
            self.existing_token.clone()
        } else if self.use_env_token {
            Some("${COVEN_TOKEN}".to_string())
        } else if self.gateway_token.is_empty() {
            None
        } else {
            Some(self.gateway_token.clone())
        }
    }

    fn generate_config(&self) -> String {
        let password = escape_toml_string(&self.get_password_value());
        let recovery_key = self.get_recovery_key_value();
        let token = self.get_token_value();

        let mut out = String::new();
        out.push_str("# Coven Matrix Bridge Configuration\n");
        out.push_str("# Generated by coven-matrix-bridge --setup\n\n");

        out.push_str("[matrix]\n");
        out.push_str(&format!(
            "homeserver = \"{}\"\n",
            escape_toml_string(&self.homeserver)
        ));
        out.push_str(&format!(
            "username = \"{}\"\n",
            escape_toml_string(&self.username)
        ));
        out.push_str(&format!("password = \"{}\"\n", password));
        if let Some(ref rk) = recovery_key {
            out.push_str(&format!("recovery_key = \"{}\"\n", escape_toml_string(rk)));
        }
        out.push('\n');

        out.push_str("[gateway]\n");
        out.push_str(&format!(
            "host = \"{}\"\n",
            escape_toml_string(&self.gateway_host)
        ));
        out.push_str(&format!("port = {}\n", self.gateway_port));
        out.push_str(&format!("tls = {}\n", self.gateway_tls));
        if let Some(t) = token {
            out.push_str(&format!("token = \"{}\"\n", escape_toml_string(&t)));
        }
        out.push('\n');

        out.push_str("[bridge]\n");
        if self.restrict_rooms && !self.allowed_rooms.trim().is_empty() {
            out.push_str("allowed_rooms = [\n");
            for room in self
                .allowed_rooms
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                out.push_str(&format!("    \"{}\",\n", escape_toml_string(room)));
            }
            out.push_str("]\n");
        } else {
            out.push_str("allowed_rooms = []\n");
        }

        if self.restrict_senders && !self.allowed_senders.trim().is_empty() {
            out.push_str("allowed_senders = [\n");
            for sender in self
                .allowed_senders
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                out.push_str(&format!("    \"{}\",\n", escape_toml_string(sender)));
            }
            out.push_str("]\n");
        } else {
            out.push_str("allowed_senders = []\n");
        }

        if self.use_prefix && !self.command_prefix.is_empty() {
            out.push_str(&format!(
                "command_prefix = \"{}\"\n",
                escape_toml_string(&self.command_prefix)
            ));
        }

        out.push_str(&format!("typing_indicator = {}\n", self.typing_indicator));

        out
    }

    fn save_config(&mut self) -> std::result::Result<(), String> {
        let config_dir =
            xdg_config_dir().ok_or_else(|| "Could not determine config directory".to_string())?;

        fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;

        let config_path = config_dir.join("matrix-bridge.toml");
        let content = self.generate_config();

        // Validate the generated TOML parses correctly
        let _: toml::Value =
            toml::from_str(&content).map_err(|e| format!("Generated invalid config: {}", e))?;

        fs::write(&config_path, &content).map_err(|e| format!("Failed to write config: {}", e))?;

        self.saved_path = Some(config_path.display().to_string());
        Ok(())
    }
}

/// Escape a string for TOML basic string format.
fn escape_toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Run the interactive TUI setup wizard.
pub fn run_setup() -> Result<()> {
    enable_raw_mode().map_err(|e| BridgeError::Config(format!("Terminal error: {}", e)))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| BridgeError::Config(format!("Terminal error: {}", e)))?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| BridgeError::Config(format!("Terminal error: {}", e)))?;

    let mut app = WizardApp::new();

    loop {
        terminal
            .draw(|f| draw_ui(f, &app))
            .map_err(|e| BridgeError::Config(format!("Draw error: {}", e)))?;

        if event::poll(std::time::Duration::from_millis(100))
            .map_err(|e| BridgeError::Config(format!("Event error: {}", e)))?
        {
            if let Event::Key(key) =
                event::read().map_err(|e| BridgeError::Config(format!("Event error: {}", e)))?
            {
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

    if let Some(path) = app.saved_path {
        println!();
        println!("  Configuration saved to: {}", path);
        println!();

        let needs_env_vars = app.use_env_password || (app.use_env_token && !app.use_existing_token);
        if needs_env_vars {
            println!("  Before running, set environment variables:");
            if app.use_env_password {
                println!("    export MATRIX_PASSWORD='your-password'");
            }
            if app.use_env_token && !app.use_existing_token {
                println!("    export COVEN_TOKEN='your-token'");
            }
            println!();
        }

        println!("  Run the bridge with:");
        println!("    coven-matrix-bridge");
        println!();
    }

    Ok(())
}

fn handle_key_input(key: KeyCode, app: &mut WizardApp) {
    match app.step {
        WizardStep::Matrix => handle_matrix_input(key, app),
        WizardStep::Gateway => handle_gateway_input(key, app),
        WizardStep::AccessControl => handle_access_input(key, app),
        WizardStep::Review => match key {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app.next_step(),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app.prev_step(),
            _ => {}
        },
    }
}

fn handle_matrix_input(key: KeyCode, app: &mut WizardApp) {
    match key {
        KeyCode::Enter => app.next_step(),
        KeyCode::Esc => app.prev_step(),
        KeyCode::Tab | KeyCode::Down => {
            app.matrix_focus = match app.matrix_focus {
                MatrixField::Homeserver => MatrixField::Username,
                MatrixField::Username => MatrixField::Password,
                MatrixField::Password => MatrixField::RecoveryKey,
                MatrixField::RecoveryKey => MatrixField::Homeserver,
            };
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.matrix_focus = match app.matrix_focus {
                MatrixField::Homeserver => MatrixField::RecoveryKey,
                MatrixField::Username => MatrixField::Homeserver,
                MatrixField::Password => MatrixField::Username,
                MatrixField::RecoveryKey => MatrixField::Password,
            };
        }
        KeyCode::Char('e') if app.matrix_focus == MatrixField::Password => {
            app.use_env_password = !app.use_env_password;
            app.error_message = None;
        }
        KeyCode::Char('e') if app.matrix_focus == MatrixField::RecoveryKey => {
            app.use_env_recovery_key = !app.use_env_recovery_key;
            app.error_message = None;
        }
        KeyCode::Char(c) => {
            app.error_message = None;
            match app.matrix_focus {
                MatrixField::Homeserver => app.homeserver.push(c),
                MatrixField::Username => app.username.push(c),
                MatrixField::Password if !app.use_env_password => app.password.push(c),
                MatrixField::RecoveryKey if !app.use_env_recovery_key => app.recovery_key.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            app.error_message = None;
            match app.matrix_focus {
                MatrixField::Homeserver => {
                    app.homeserver.pop();
                }
                MatrixField::Username => {
                    app.username.pop();
                }
                MatrixField::Password if !app.use_env_password => {
                    app.password.pop();
                }
                MatrixField::RecoveryKey if !app.use_env_recovery_key => {
                    app.recovery_key.pop();
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_gateway_input(key: KeyCode, app: &mut WizardApp) {
    match key {
        KeyCode::Enter => app.next_step(),
        KeyCode::Esc => app.prev_step(),
        KeyCode::Tab | KeyCode::Down => {
            app.gateway_focus = match app.gateway_focus {
                GatewayField::Host => GatewayField::Port,
                GatewayField::Port => GatewayField::Tls,
                GatewayField::Tls => GatewayField::Token,
                GatewayField::Token => GatewayField::Host,
            };
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.gateway_focus = match app.gateway_focus {
                GatewayField::Host => GatewayField::Token,
                GatewayField::Port => GatewayField::Host,
                GatewayField::Tls => GatewayField::Port,
                GatewayField::Token => GatewayField::Tls,
            };
        }
        KeyCode::Char(' ') if app.gateway_focus == GatewayField::Tls => {
            app.gateway_tls = !app.gateway_tls;
        }
        KeyCode::Char('e') if app.gateway_focus == GatewayField::Token => {
            if app.existing_token.is_some() {
                // Cycle: existing -> env -> manual -> existing
                if app.use_existing_token {
                    app.use_existing_token = false;
                    app.use_env_token = true;
                } else if app.use_env_token {
                    app.use_env_token = false;
                } else {
                    app.use_existing_token = true;
                }
            } else {
                app.use_env_token = !app.use_env_token;
            }
        }
        KeyCode::Char(c) => {
            app.error_message = None;
            match app.gateway_focus {
                GatewayField::Host => app.gateway_host.push(c),
                GatewayField::Port if c.is_ascii_digit() => app.gateway_port.push(c),
                GatewayField::Token if !app.use_env_token && !app.use_existing_token => {
                    app.gateway_token.push(c);
                }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            app.error_message = None;
            match app.gateway_focus {
                GatewayField::Host => {
                    app.gateway_host.pop();
                }
                GatewayField::Port => {
                    app.gateway_port.pop();
                }
                GatewayField::Token if !app.use_env_token && !app.use_existing_token => {
                    app.gateway_token.pop();
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_access_input(key: KeyCode, app: &mut WizardApp) {
    const FIELD_COUNT: usize = 6;

    match key {
        KeyCode::Enter => app.next_step(),
        KeyCode::Esc => app.prev_step(),
        KeyCode::Tab | KeyCode::Down => {
            app.access_focus = (app.access_focus + 1) % FIELD_COUNT;
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.access_focus = (app.access_focus + FIELD_COUNT - 1) % FIELD_COUNT;
        }
        KeyCode::Char(' ') => match app.access_focus {
            0 => app.restrict_rooms = !app.restrict_rooms,
            2 => app.restrict_senders = !app.restrict_senders,
            4 => app.use_prefix = !app.use_prefix,
            5 => app.typing_indicator = !app.typing_indicator,
            _ => {}
        },
        KeyCode::Char(c) => match app.access_focus {
            1 if app.restrict_rooms => app.allowed_rooms.push(c),
            3 if app.restrict_senders => app.allowed_senders.push(c),
            4 if app.use_prefix => app.command_prefix.push(c),
            _ => {}
        },
        KeyCode::Backspace => match app.access_focus {
            1 if app.restrict_rooms => {
                app.allowed_rooms.pop();
            }
            3 if app.restrict_senders => {
                app.allowed_senders.pop();
            }
            4 if app.use_prefix => {
                app.command_prefix.pop();
            }
            _ => {}
        },
        _ => {}
    }
}

fn draw_ui(f: &mut Frame, app: &WizardApp) {
    let size = f.area();
    let popup_area = centered_rect(70, 90, size);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Matrix Bridge Setup ",
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
            Constraint::Length(2), // Spacer
            Constraint::Min(20),   // Content
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Error
            Constraint::Length(2), // Controls
        ])
        .split(popup_area);

    // Step indicator
    let step_text = format!(
        "Step {} of 4: {}",
        app.step_number(),
        match app.step {
            WizardStep::Matrix => "Matrix Homeserver",
            WizardStep::Gateway => "Coven Gateway",
            WizardStep::AccessControl => "Access Control",
            WizardStep::Review => "Review",
        }
    );
    let step_para = Paragraph::new(step_text).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(step_para, inner[0]);

    // Content
    match app.step {
        WizardStep::Matrix => draw_matrix_step(f, app, inner[2]),
        WizardStep::Gateway => draw_gateway_step(f, app, inner[2]),
        WizardStep::AccessControl => draw_access_step(f, app, inner[2]),
        WizardStep::Review => draw_review_step(f, app, inner[2]),
    }

    // Error message
    if let Some(ref error) = app.error_message {
        let error_para =
            Paragraph::new(format!(" {}", error)).style(Style::default().fg(Color::Red));
        f.render_widget(error_para, inner[4]);
    }

    // Controls
    let controls = match app.step {
        WizardStep::Matrix => "[Tab/] Navigate  [E] Toggle env var  [Enter] Next  [Esc] Cancel",
        WizardStep::Gateway => "[Tab/] Navigate  [Space] Toggle TLS  [E] Token mode  [Enter] Next",
        WizardStep::AccessControl => "[Tab/] Navigate  [Space] Toggle  [Enter] Next  [Esc] Back",
        WizardStep::Review => "[Enter/Y] Save  [Esc/N] Go Back",
    };
    let controls_para = Paragraph::new(controls).style(Style::default().fg(Color::DarkGray));
    f.render_widget(controls_para, inner[5]);
}

fn draw_matrix_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Homeserver
            Constraint::Length(1), // Spacer
            Constraint::Length(4), // Username
            Constraint::Length(1), // Spacer
            Constraint::Length(4), // Password
            Constraint::Length(1), // Spacer
            Constraint::Length(4), // Recovery Key
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Help
        ])
        .split(area);

    draw_text_field(
        f,
        chunks[0],
        "Homeserver URL",
        &app.homeserver,
        app.matrix_focus == MatrixField::Homeserver,
        false,
    );
    draw_text_field(
        f,
        chunks[2],
        "Bot Username",
        &app.username,
        app.matrix_focus == MatrixField::Username,
        false,
    );

    let password_label = if app.use_env_password {
        "Password [E: env var]"
    } else {
        "Password [E: manual]"
    };
    let password_value = if app.use_env_password {
        "${MATRIX_PASSWORD}".to_string()
    } else {
        "*".repeat(app.password.len())
    };
    draw_text_field(
        f,
        chunks[4],
        password_label,
        &password_value,
        app.matrix_focus == MatrixField::Password,
        app.use_env_password,
    );

    let recovery_label = if app.use_env_recovery_key {
        "Recovery Key [E: env var]"
    } else {
        "Recovery Key [E: manual]"
    };
    let recovery_value = if app.use_env_recovery_key {
        "${MATRIX_RECOVERY_KEY}".to_string()
    } else if app.recovery_key.is_empty() {
        "(optional)".to_string()
    } else {
        "*".repeat(app.recovery_key.len())
    };
    draw_text_field(
        f,
        chunks[6],
        recovery_label,
        &recovery_value,
        app.matrix_focus == MatrixField::RecoveryKey,
        app.use_env_recovery_key,
    );

    let help = Paragraph::new("Press [E] to toggle environment variable mode for secrets")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[8]);
}

fn draw_gateway_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Host
            Constraint::Length(1), // Spacer
            Constraint::Length(4), // Port
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // TLS
            Constraint::Length(1), // Spacer
            Constraint::Length(4), // Token
            Constraint::Length(1), // Spacer
            Constraint::Min(1),    // Help
        ])
        .split(area);

    // Show if loaded from existing config
    let host_label = if app.existing_token.is_some() {
        "Gateway Host (from coven config)"
    } else {
        "Gateway Host"
    };
    draw_text_field(
        f,
        chunks[0],
        host_label,
        &app.gateway_host,
        app.gateway_focus == GatewayField::Host,
        false,
    );
    draw_text_field(
        f,
        chunks[2],
        "Gateway Port",
        &app.gateway_port,
        app.gateway_focus == GatewayField::Port,
        false,
    );

    // TLS checkbox
    let tls_focused = app.gateway_focus == GatewayField::Tls;
    let tls_checkbox = if app.gateway_tls { "[x]" } else { "[ ]" };
    let tls_style = if tls_focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };
    let tls_line = Line::from(vec![Span::styled(
        format!("{} Use TLS (https)", tls_checkbox),
        tls_style,
    )]);
    f.render_widget(Paragraph::new(tls_line), chunks[4]);

    // Token field
    let token_label = if app.use_existing_token {
        "Token [E: from coven config]"
    } else if app.use_env_token {
        "Token [E: env var]"
    } else {
        "Token [E: manual]"
    };
    let token_value = if app.use_existing_token {
        "(using existing token)".to_string()
    } else if app.use_env_token {
        "${COVEN_TOKEN}".to_string()
    } else {
        app.gateway_token.clone()
    };
    draw_text_field(
        f,
        chunks[6],
        token_label,
        &token_value,
        app.gateway_focus == GatewayField::Token,
        app.use_env_token || app.use_existing_token,
    );

    let help_text = if app.existing_token.is_some() {
        "Press [E] to cycle: existing config -> env var -> manual"
    } else {
        "Press [E] to toggle environment variable mode for token"
    };
    let help = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[8]);
}

fn draw_access_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Restrict rooms checkbox
            Constraint::Length(4), // Rooms input
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Restrict senders checkbox
            Constraint::Length(4), // Senders input
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Prefix checkbox
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Typing checkbox
            Constraint::Min(1),    // Help
        ])
        .split(area);

    // Room restriction
    draw_checkbox(
        f,
        chunks[0],
        "Restrict to specific rooms",
        app.restrict_rooms,
        app.access_focus == 0,
    );
    if app.restrict_rooms {
        draw_text_field(
            f,
            chunks[1],
            "Room IDs (comma-separated)",
            &app.allowed_rooms,
            app.access_focus == 1,
            false,
        );
    }

    // Sender restriction
    draw_checkbox(
        f,
        chunks[3],
        "Restrict to specific users",
        app.restrict_senders,
        app.access_focus == 2,
    );
    if app.restrict_senders {
        draw_text_field(
            f,
            chunks[4],
            "User IDs (comma-separated)",
            &app.allowed_senders,
            app.access_focus == 3,
            false,
        );
    }

    // Prefix
    let prefix_label = if app.use_prefix {
        format!("Require prefix: \"{}\"", app.command_prefix)
    } else {
        "Require message prefix".to_string()
    };
    draw_checkbox(
        f,
        chunks[6],
        &prefix_label,
        app.use_prefix,
        app.access_focus == 4,
    );

    // Typing indicator
    draw_checkbox(
        f,
        chunks[8],
        "Show typing indicator",
        app.typing_indicator,
        app.access_focus == 5,
    );
}

fn draw_review_step(f: &mut Frame, app: &WizardApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Label
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Homeserver
            Constraint::Length(2), // Username
            Constraint::Length(2), // Gateway
            Constraint::Length(2), // Token mode
            Constraint::Length(2), // Access
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
        ("Homeserver:", app.homeserver.clone()),
        ("Username:", app.username.clone()),
        (
            "Gateway:",
            format!(
                "{}:{} (TLS: {})",
                app.gateway_host, app.gateway_port, app.gateway_tls
            ),
        ),
        (
            "Token:",
            if app.use_existing_token {
                "from coven config".to_string()
            } else if app.use_env_token {
                "from env var".to_string()
            } else {
                "manual".to_string()
            },
        ),
        (
            "Access:",
            format!(
                "rooms: {}, senders: {}",
                if app.restrict_rooms {
                    "restricted"
                } else {
                    "all"
                },
                if app.restrict_senders {
                    "restricted"
                } else {
                    "all"
                }
            ),
        ),
    ];

    for (idx, (label, value)) in fields.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(
                format!("  {:12}", label),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(value, Style::default().fg(Color::Cyan)),
        ]);
        f.render_widget(Paragraph::new(line), chunks[2 + idx]);
    }
}

fn draw_text_field(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    dimmed: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let label_style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(format!("{}:", label)).style(label_style),
        chunks[0],
    );

    let value_style = if dimmed {
        Style::default().fg(Color::DarkGray)
    } else if focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Gray)
    };

    let display = if focused && !dimmed {
        format!("{}_", value)
    } else {
        value.to_string()
    };

    let border_color = if focused {
        Color::Green
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    f.render_widget(
        Paragraph::new(display).style(value_style).block(block),
        Rect::new(area.x, area.y + 1, area.width.min(50), 3),
    );
}

fn draw_checkbox(f: &mut Frame, area: Rect, label: &str, checked: bool, focused: bool) {
    let checkbox = if checked { "[x]" } else { "[ ]" };
    let style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };
    let line = Line::from(vec![Span::styled(format!("{} {}", checkbox, label), style)]);
    f.render_widget(Paragraph::new(line), area);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_toml_string_basic() {
        assert_eq!(escape_toml_string("hello"), "hello");
        assert_eq!(escape_toml_string(""), "");
    }

    #[test]
    fn test_escape_toml_string_quotes() {
        assert_eq!(escape_toml_string(r#"say "hello""#), r#"say \"hello\""#);
    }

    #[test]
    fn test_escape_toml_string_backslashes() {
        assert_eq!(escape_toml_string(r"path\to\file"), r"path\\to\\file");
    }

    #[test]
    fn test_escape_toml_string_mixed() {
        assert_eq!(
            escape_toml_string(r#"password\"with\special"#),
            r#"password\\\"with\\special"#
        );
    }

    #[test]
    fn test_wizard_app_prefills_from_existing() {
        // This tests that default values are set correctly
        let app = WizardApp::new();
        // These should have defaults even without existing config
        assert!(!app.homeserver.is_empty());
        // Gateway port should be a valid port (either default 6666 or loaded from config)
        assert!(!app.gateway_port.is_empty());
        assert!(app.gateway_port.parse::<u16>().is_ok());
    }
}
