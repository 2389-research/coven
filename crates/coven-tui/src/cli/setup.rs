// ABOUTME: Interactive TUI setup wizard for first-time users.
// ABOUTME: Form-based configuration for gateway host/port and theme selection.

use coven_grpc::ChannelConfig;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::time::Duration;
use tui_textarea::TextArea;

use crate::error::Result;
use crate::state::config::Config;
use crate::theme;
use crate::tui::Tui;

/// Which field is currently focused in the setup form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupField {
    Host,
    Port,
    UseTls,
    Theme,
    TestButton,
    SaveButton,
}

impl SetupField {
    fn next(self) -> Self {
        match self {
            Self::Host => Self::Port,
            Self::Port => Self::UseTls,
            Self::UseTls => Self::Theme,
            Self::Theme => Self::TestButton,
            Self::TestButton => Self::SaveButton,
            Self::SaveButton => Self::Host,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Host => Self::SaveButton,
            Self::Port => Self::Host,
            Self::UseTls => Self::Port,
            Self::Theme => Self::UseTls,
            Self::TestButton => Self::Theme,
            Self::SaveButton => Self::TestButton,
        }
    }
}

/// Connection test status.
#[derive(Debug, Clone)]
enum ConnectionStatus {
    NotTested,
    Testing,
    Success,
    Failed(String),
}

/// State for the setup form.
struct SetupForm<'a> {
    host: TextArea<'a>,
    port: TextArea<'a>,
    use_tls: bool,
    themes: Vec<&'static str>,
    selected_theme: usize,
    focus: SetupField,
    connection_status: ConnectionStatus,
    should_save: bool,
    should_quit: bool,
}

impl<'a> SetupForm<'a> {
    fn new() -> Self {
        let themes = theme::list_themes();

        let mut host = TextArea::default();
        host.insert_str("localhost");

        let mut port = TextArea::default();
        port.insert_str("50051");

        Self {
            host,
            port,
            use_tls: false,
            themes: themes.to_vec(),
            selected_theme: 0,
            focus: SetupField::Host,
            connection_status: ConnectionStatus::NotTested,
            should_save: false,
            should_quit: false,
        }
    }

    fn gateway_url(&self) -> String {
        let host = self
            .host
            .lines()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("localhost");
        let port = self
            .port
            .lines()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("50051");
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, host, port)
    }

    fn host_value(&self) -> String {
        self.host
            .lines()
            .first()
            .cloned()
            .unwrap_or_else(|| "localhost".to_string())
    }

    fn port_value(&self) -> u16 {
        self.port
            .lines()
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50051)
    }

    fn handle_key(&mut self, key: event::KeyEvent) {
        // Global keys
        if key.code == KeyCode::Esc {
            self.should_quit = true;
            return;
        }

        if key.code == KeyCode::Tab {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                self.focus = self.focus.prev();
            } else {
                self.focus = self.focus.next();
            }
            return;
        }

        // Field-specific handling
        match self.focus {
            SetupField::Host => {
                if key.code == KeyCode::Enter {
                    self.focus = self.focus.next();
                } else {
                    self.host.input(key);
                }
            }
            SetupField::Port => {
                if key.code == KeyCode::Enter {
                    self.focus = self.focus.next();
                } else {
                    // Only allow digits
                    match key.code {
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            self.port.input(key);
                        }
                        KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => {
                            self.port.input(key);
                        }
                        _ => {}
                    }
                }
            }
            SetupField::UseTls => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.use_tls = !self.use_tls;
                }
                KeyCode::Tab => {
                    self.focus = self.focus.next();
                }
                _ => {}
            },
            SetupField::Theme => match key.code {
                KeyCode::Up => {
                    if self.selected_theme > 0 {
                        self.selected_theme -= 1;
                    }
                }
                KeyCode::Down => {
                    if self.selected_theme < self.themes.len() - 1 {
                        self.selected_theme += 1;
                    }
                }
                KeyCode::Enter => {
                    self.focus = self.focus.next();
                }
                _ => {}
            },
            SetupField::TestButton => {
                if key.code == KeyCode::Enter {
                    // Mark as testing - actual test happens in event loop
                    self.connection_status = ConnectionStatus::Testing;
                }
            }
            SetupField::SaveButton => {
                if key.code == KeyCode::Enter {
                    self.should_save = true;
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Dark background
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Rgb(20, 20, 30))),
            area,
        );

        // Center the form
        let form_width = 50.min(area.width.saturating_sub(4));
        let form_height = 20.min(area.height.saturating_sub(4));
        let form_x = (area.width - form_width) / 2;
        let form_y = (area.height - form_height) / 2;
        let form_area = Rect::new(form_x, form_y, form_width, form_height);

        // Form container
        let form_block = Block::default()
            .title(" Fold Gateway Setup ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 140, 255)))
            .style(Style::default().bg(Color::Rgb(25, 25, 40)));
        frame.render_widget(form_block, form_area);

        let inner = Rect::new(form_x + 2, form_y + 2, form_width - 4, form_height - 4);

        // Layout fields vertically
        let mut y = inner.y;

        // Section: gRPC Gateway
        let label =
            Paragraph::new("gRPC Gateway").style(Style::default().fg(Color::Rgb(140, 140, 160)));
        frame.render_widget(label, Rect::new(inner.x, y, inner.width, 1));
        y += 2;

        // Host field
        self.render_text_field(
            frame,
            inner.x,
            y,
            inner.width,
            "Host",
            &self.host,
            self.focus == SetupField::Host,
        );
        y += 3;

        // Port field
        self.render_text_field(
            frame,
            inner.x,
            y,
            inner.width,
            "Port",
            &self.port,
            self.focus == SetupField::Port,
        );
        y += 3;

        // TLS checkbox
        self.render_checkbox(
            frame,
            inner.x,
            y,
            "Use TLS (https)",
            self.use_tls,
            self.focus == SetupField::UseTls,
        );
        y += 2;

        // Section: Theme
        let label = Paragraph::new("Theme").style(Style::default().fg(Color::Rgb(140, 140, 160)));
        frame.render_widget(label, Rect::new(inner.x, y, inner.width, 1));
        y += 1;

        // Theme list
        self.render_theme_list(frame, inner.x, y, inner.width, 4);
        y += 5;

        // Connection status
        let status_text = match &self.connection_status {
            ConnectionStatus::NotTested => "".to_string(),
            ConnectionStatus::Testing => "Testing connection...".to_string(),
            ConnectionStatus::Success => "Connected successfully!".to_string(),
            ConnectionStatus::Failed(e) => format!("Failed: {}", e),
        };
        let status_color = match &self.connection_status {
            ConnectionStatus::NotTested => Color::Rgb(140, 140, 160),
            ConnectionStatus::Testing => Color::Rgb(255, 200, 100),
            ConnectionStatus::Success => Color::Rgb(100, 220, 140),
            ConnectionStatus::Failed(_) => Color::Rgb(255, 100, 120),
        };
        let status = Paragraph::new(status_text).style(Style::default().fg(status_color));
        frame.render_widget(status, Rect::new(inner.x, y, inner.width, 1));
        y += 2;

        // Buttons
        self.render_button(
            frame,
            inner.x,
            y,
            16,
            "Test Connection",
            self.focus == SetupField::TestButton,
        );
        self.render_button(
            frame,
            inner.x + 18,
            y,
            12,
            "Save & Exit",
            self.focus == SetupField::SaveButton,
        );
        y += 2;

        // Help text
        let help = Paragraph::new("Tab: Next field │ Enter: Select │ Esc: Cancel")
            .style(Style::default().fg(Color::Rgb(100, 100, 120)));
        frame.render_widget(help, Rect::new(inner.x, y, inner.width, 1));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_text_field(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        width: u16,
        label: &str,
        textarea: &TextArea,
        focused: bool,
    ) {
        let label_para = Paragraph::new(format!("{}:", label))
            .style(Style::default().fg(Color::Rgb(180, 180, 200)));
        frame.render_widget(label_para, Rect::new(x, y, width, 1));

        let border_color = if focused {
            Color::Rgb(100, 180, 255)
        } else {
            Color::Rgb(60, 60, 80)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(Color::Rgb(30, 30, 45)));

        // Render textarea content
        let content = textarea.lines().first().map(|s| s.as_str()).unwrap_or("");
        let para = Paragraph::new(content)
            .block(block)
            .style(Style::default().fg(Color::Rgb(230, 230, 240)));
        frame.render_widget(para, Rect::new(x, y + 1, width.min(30), 3));
    }

    fn render_theme_list(&self, frame: &mut Frame, x: u16, y: u16, width: u16, height: u16) {
        let focused = self.focus == SetupField::Theme;
        let border_color = if focused {
            Color::Rgb(100, 180, 255)
        } else {
            Color::Rgb(60, 60, 80)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(Color::Rgb(30, 30, 45)));

        frame.render_widget(block, Rect::new(x, y, width.min(30), height + 2));

        // Render theme items
        for (i, theme_name) in self.themes.iter().enumerate().take(height as usize) {
            let is_selected = i == self.selected_theme;
            let prefix = if is_selected { "> " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(60, 60, 100))
            } else {
                Style::default().fg(Color::Rgb(180, 180, 200))
            };
            let item = Paragraph::new(format!("{}{}", prefix, theme_name)).style(style);
            frame.render_widget(
                item,
                Rect::new(x + 1, y + 1 + i as u16, width.min(30) - 2, 1),
            );
        }
    }

    fn render_button(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        width: u16,
        label: &str,
        focused: bool,
    ) {
        let style = if focused {
            Style::default()
                .fg(Color::Rgb(20, 20, 30))
                .bg(Color::Rgb(100, 180, 255))
        } else {
            Style::default()
                .fg(Color::Rgb(180, 180, 200))
                .bg(Color::Rgb(50, 50, 70))
        };
        let button = Paragraph::new(format!(" {} ", label))
            .style(style)
            .alignment(Alignment::Center);
        frame.render_widget(button, Rect::new(x, y, width, 1));
    }

    fn render_checkbox(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        label: &str,
        checked: bool,
        focused: bool,
    ) {
        let checkbox = if checked { "[✓]" } else { "[ ]" };
        let border_color = if focused {
            Color::Rgb(100, 180, 255)
        } else {
            Color::Rgb(60, 60, 80)
        };
        let text_color = if focused {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(180, 180, 200)
        };

        let line = Line::from(vec![
            Span::styled(checkbox, Style::default().fg(border_color)),
            Span::raw(" "),
            Span::styled(label, Style::default().fg(text_color)),
        ]);
        let para = Paragraph::new(line);
        frame.render_widget(para, Rect::new(x, y, 30, 1));
    }
}

/// Run the interactive TUI setup wizard.
pub async fn run() -> Result<()> {
    let mut tui = Tui::new()?;
    let mut form = SetupForm::new();

    loop {
        // Draw
        tui.terminal_mut().draw(|frame| form.render(frame))?;

        // Handle connection test if in progress
        if matches!(form.connection_status, ConnectionStatus::Testing) {
            let url = form.gateway_url();
            form.connection_status = test_connection(&url).await;
            continue;
        }

        // Handle save
        if form.should_save {
            let mut config = Config::default();
            config.gateway.host = form.host_value();
            config.gateway.port = form.port_value();
            config.gateway.use_tls = form.use_tls;
            config.appearance.theme = form.themes[form.selected_theme].to_string();
            config.save()?;

            // Exit TUI and show confirmation
            drop(tui);
            let config_path = Config::config_path()?;
            println!();
            println!("Configuration saved to {}", config_path.display());
            println!();
            println!("Run 'coven-chat' to launch the TUI!");
            return Ok(());
        }

        // Handle quit
        if form.should_quit {
            return Ok(());
        }

        // Poll for input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                form.handle_key(key);
            }
        }
    }
}

/// Test connection to the gateway.
async fn test_connection(url: &str) -> ConnectionStatus {
    // Use async channel creation directly to avoid blocking inside async context.
    // CovenClient::check_health() uses block_on internally which panics in async.
    let config = ChannelConfig::new(url).with_connect_timeout(std::time::Duration::from_secs(5));
    match coven_grpc::create_channel(&config).await {
        Ok(_) => ConnectionStatus::Success,
        Err(e) => ConnectionStatus::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_list_is_non_empty() {
        let themes = theme::list_themes();
        assert!(!themes.is_empty());
        assert!(themes.contains(&"default"));
    }

    #[test]
    fn test_default_gateway_url() {
        let form = SetupForm::new();
        assert_eq!(form.gateway_url(), "http://localhost:50051");
    }

    #[test]
    fn test_setup_field_navigation() {
        assert_eq!(SetupField::Host.next(), SetupField::Port);
        assert_eq!(SetupField::Port.next(), SetupField::UseTls);
        assert_eq!(SetupField::UseTls.next(), SetupField::Theme);
        assert_eq!(SetupField::Theme.next(), SetupField::TestButton);
        assert_eq!(SetupField::TestButton.next(), SetupField::SaveButton);
        assert_eq!(SetupField::SaveButton.next(), SetupField::Host);

        assert_eq!(SetupField::Host.prev(), SetupField::SaveButton);
        assert_eq!(SetupField::Port.prev(), SetupField::Host);
        assert_eq!(SetupField::UseTls.prev(), SetupField::Port);
        assert_eq!(SetupField::Theme.prev(), SetupField::UseTls);
    }

    #[test]
    fn test_form_initial_state() {
        let form = SetupForm::new();
        assert_eq!(form.focus, SetupField::Host);
        assert_eq!(form.selected_theme, 0);
        assert!(matches!(
            form.connection_status,
            ConnectionStatus::NotTested
        ));
        assert!(!form.should_save);
        assert!(!form.should_quit);
    }
}
