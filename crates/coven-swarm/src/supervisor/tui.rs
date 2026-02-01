// ABOUTME: TUI display for supervisor showing workspace agent activity.
// ABOUTME: Uses ratatui for terminal rendering with color-coded log blocks.

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Events that can be displayed in the TUI
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Agent spawned
    AgentSpawned { workspace: String, pid: u32 },
    /// Agent registered with gateway
    AgentRegistered {
        workspace: String,
        instance_id: String,
    },
    /// Agent received a message
    AgentMessage {
        workspace: String,
        request_id: String,
    },
    /// Agent log output
    AgentLog { workspace: String, line: String },
    /// Agent error
    AgentError { workspace: String, error: String },
    /// Agent exited
    AgentExited {
        workspace: String,
        code: Option<i32>,
    },
    /// Socket command received
    SocketCommand { command: String },
    /// System message
    System { message: String },
}

/// Log entry with metadata
#[derive(Debug, Clone)]
struct LogEntry {
    timestamp: Instant,
    workspace: Option<String>,
    kind: LogKind,
    content: String,
}

#[derive(Debug, Clone, Copy)]
enum LogKind {
    System,
    Spawned,
    Registered,
    Message,
    Log,
    Error,
    Exited,
    Socket,
}

impl LogKind {
    fn color(&self) -> Color {
        match self {
            LogKind::System => Color::Cyan,
            LogKind::Spawned => Color::Green,
            LogKind::Registered => Color::Blue,
            LogKind::Message => Color::Yellow,
            LogKind::Log => Color::White,
            LogKind::Error => Color::Red,
            LogKind::Exited => Color::Magenta,
            LogKind::Socket => Color::Gray,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            LogKind::System => "SYSTEM",
            LogKind::Spawned => "SPAWN",
            LogKind::Registered => "REGISTER",
            LogKind::Message => "MSG",
            LogKind::Log => "LOG",
            LogKind::Error => "ERROR",
            LogKind::Exited => "EXIT",
            LogKind::Socket => "SOCKET",
        }
    }
}

/// Agent state for display
#[derive(Debug, Clone)]
struct AgentState {
    workspace: String,
    pid: Option<u32>,
    instance_id: Option<String>,
    status: AgentStatus,
    last_activity: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AgentStatus {
    Starting,
    Connected,
    Active,
    Error,
    Exited,
}

impl AgentStatus {
    fn color(&self) -> Color {
        match self {
            AgentStatus::Starting => Color::Yellow,
            AgentStatus::Connected => Color::Green,
            AgentStatus::Active => Color::Cyan,
            AgentStatus::Error => Color::Red,
            AgentStatus::Exited => Color::Gray,
        }
    }

    fn symbol(&self) -> &'static str {
        match self {
            AgentStatus::Starting => "o",
            AgentStatus::Connected => "*",
            AgentStatus::Active => "@",
            AgentStatus::Error => "x",
            AgentStatus::Exited => "-",
        }
    }
}

/// TUI state
pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    agents: HashMap<String, AgentState>,
    logs: Vec<LogEntry>,
    scroll_offset: usize,
    max_logs: usize,
    event_rx: mpsc::Receiver<TuiEvent>,
    start_time: Instant,
}

impl Tui {
    /// Create a new TUI and return the event sender
    pub fn new() -> io::Result<(Self, mpsc::Sender<TuiEvent>)> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let (event_tx, event_rx) = mpsc::channel(256);

        Ok((
            Self {
                terminal,
                agents: HashMap::new(),
                logs: Vec::new(),
                scroll_offset: 0,
                max_logs: 1000,
                event_rx,
                start_time: Instant::now(),
            },
            event_tx,
        ))
    }

    /// Run the TUI event loop
    pub async fn run(mut self) -> io::Result<()> {
        loop {
            // Draw - extract data to avoid borrow issues
            let agents = self.agents.clone();
            let logs = self.logs.clone();
            let scroll_offset = self.scroll_offset;
            let start_time = self.start_time;

            self.terminal.draw(|f| {
                render_ui(f, &agents, &logs, scroll_offset, start_time);
            })?;

            // Check for keyboard input (non-blocking)
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Char('q'), _) => break,
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            self.scroll_offset = self.scroll_offset.saturating_add(1);
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            self.scroll_offset = self.scroll_offset.saturating_sub(1);
                        }
                        (KeyCode::PageUp, _) => {
                            self.scroll_offset = self.scroll_offset.saturating_add(10);
                        }
                        (KeyCode::PageDown, _) => {
                            self.scroll_offset = self.scroll_offset.saturating_sub(10);
                        }
                        (KeyCode::Home, _) => {
                            self.scroll_offset = self.logs.len().saturating_sub(1);
                        }
                        (KeyCode::End, _) => {
                            self.scroll_offset = 0;
                        }
                        _ => {}
                    }
                }
            }

            // Process events
            while let Ok(event) = self.event_rx.try_recv() {
                self.handle_event(event);
            }
        }

        self.cleanup()
    }

    fn handle_event(&mut self, event: TuiEvent) {
        let (workspace, kind, content) = match event {
            TuiEvent::AgentSpawned { workspace, pid } => {
                self.agents.insert(
                    workspace.clone(),
                    AgentState {
                        workspace: workspace.clone(),
                        pid: Some(pid),
                        instance_id: None,
                        status: AgentStatus::Starting,
                        last_activity: Instant::now(),
                    },
                );
                (Some(workspace), LogKind::Spawned, format!("pid={}", pid))
            }
            TuiEvent::AgentRegistered {
                workspace,
                instance_id,
            } => {
                if let Some(agent) = self.agents.get_mut(&workspace) {
                    agent.instance_id = Some(instance_id.clone());
                    agent.status = AgentStatus::Connected;
                    agent.last_activity = Instant::now();
                }
                (
                    Some(workspace),
                    LogKind::Registered,
                    format!("instance={}", instance_id),
                )
            }
            TuiEvent::AgentMessage {
                workspace,
                request_id,
            } => {
                if let Some(agent) = self.agents.get_mut(&workspace) {
                    agent.status = AgentStatus::Active;
                    agent.last_activity = Instant::now();
                }
                (
                    Some(workspace),
                    LogKind::Message,
                    format!("request={}", request_id),
                )
            }
            TuiEvent::AgentLog { workspace, line } => {
                if let Some(agent) = self.agents.get_mut(&workspace) {
                    agent.last_activity = Instant::now();
                }
                (Some(workspace), LogKind::Log, line)
            }
            TuiEvent::AgentError { workspace, error } => {
                if let Some(agent) = self.agents.get_mut(&workspace) {
                    agent.status = AgentStatus::Error;
                    agent.last_activity = Instant::now();
                }
                (Some(workspace), LogKind::Error, error)
            }
            TuiEvent::AgentExited { workspace, code } => {
                if let Some(agent) = self.agents.get_mut(&workspace) {
                    agent.status = AgentStatus::Exited;
                    agent.last_activity = Instant::now();
                }
                let msg = match code {
                    Some(c) => format!("code={}", c),
                    None => "signal".to_string(),
                };
                (Some(workspace), LogKind::Exited, msg)
            }
            TuiEvent::SocketCommand { command } => (None, LogKind::Socket, command),
            TuiEvent::System { message } => (None, LogKind::System, message),
        };

        self.logs.push(LogEntry {
            timestamp: Instant::now(),
            workspace,
            kind,
            content,
        });

        // Trim old logs
        if self.logs.len() > self.max_logs {
            self.logs.remove(0);
        }

        // Auto-scroll to bottom when new content arrives (if already at bottom)
        if self.scroll_offset == 0 {
            // Already at bottom, stay there
        }
    }

    fn cleanup(mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Render the UI (standalone function to avoid borrow issues)
fn render_ui(
    frame: &mut Frame,
    agents: &HashMap<String, AgentState>,
    logs: &[LogEntry],
    scroll_offset: usize,
    start_time: Instant,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(5), // Agent status
            Constraint::Min(10),   // Logs
            Constraint::Length(1), // Help
        ])
        .split(frame.area());

    render_header(frame, chunks[0], agents.len(), start_time);
    render_agents(frame, chunks[1], agents);
    render_logs(frame, chunks[2], logs, scroll_offset, start_time);
    render_help(frame, chunks[3]);
}

fn render_header(frame: &mut Frame, area: Rect, agent_count: usize, start_time: Instant) {
    let elapsed = start_time.elapsed();
    let uptime = format!(
        "{:02}:{:02}:{:02}",
        elapsed.as_secs() / 3600,
        (elapsed.as_secs() % 3600) / 60,
        elapsed.as_secs() % 60
    );

    let title = Line::from(vec![
        Span::styled(
            " coven-swarm ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{} agents", agent_count),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("uptime {}", uptime),
            Style::default().fg(Color::Gray),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(title).block(block);
    frame.render_widget(paragraph, area);
}

fn render_agents(frame: &mut Frame, area: Rect, agents: &HashMap<String, AgentState>) {
    let mut items: Vec<ListItem> = agents
        .values()
        .map(|agent| {
            let status_style = Style::default().fg(agent.status.color());
            let name_style = if agent.workspace == "dispatch" {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                Span::styled(format!(" {} ", agent.status.symbol()), status_style),
                Span::styled(&agent.workspace, name_style),
                Span::raw(" "),
                Span::styled(
                    agent
                        .instance_id
                        .as_deref()
                        .unwrap_or("-")
                        .chars()
                        .take(11)
                        .collect::<String>(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    items.sort_by(|a, b| {
        // Sort dispatch first, then alphabetically
        let a_str = format!("{:?}", a);
        let b_str = format!("{:?}", b);
        if a_str.contains("dispatch") {
            std::cmp::Ordering::Less
        } else if b_str.contains("dispatch") {
            std::cmp::Ordering::Greater
        } else {
            a_str.cmp(&b_str)
        }
    });

    let block = Block::default()
        .title(" Agents ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_logs(
    frame: &mut Frame,
    area: Rect,
    logs: &[LogEntry],
    scroll_offset: usize,
    start_time: Instant,
) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_logs = logs.len();

    // Calculate visible range with scroll offset
    let end = total_logs.saturating_sub(scroll_offset);
    let start = end.saturating_sub(inner_height);

    let items: Vec<ListItem> = logs[start..end]
        .iter()
        .map(|entry| {
            let elapsed = entry.timestamp.duration_since(start_time);
            let timestamp = format!(
                "{:02}:{:02}:{:02}",
                (elapsed.as_secs() / 60) % 60,
                elapsed.as_secs() % 60,
                (elapsed.as_millis() % 1000) / 10
            );

            let workspace_span = match &entry.workspace {
                Some(ws) => Span::styled(
                    format!("[{}]", ws),
                    Style::default().fg(if ws == "dispatch" {
                        Color::Yellow
                    } else {
                        Color::Blue
                    }),
                ),
                None => Span::styled("[*]", Style::default().fg(Color::Gray)),
            };

            let line = Line::from(vec![
                Span::styled(timestamp, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("{:8}", entry.kind.label()),
                    Style::default().fg(entry.kind.color()),
                ),
                Span::raw(" "),
                workspace_span,
                Span::raw(" "),
                Span::raw(&entry.content),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = if scroll_offset > 0 {
        format!(" Logs (scroll: -{}) ", scroll_offset)
    } else {
        " Logs ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::styled("^/k", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll up  "),
        Span::styled("v/j", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll down  "),
        Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
        Span::raw(" page  "),
        Span::styled("Home/End", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom"),
    ]);

    let paragraph = Paragraph::new(help);
    frame.render_widget(paragraph, area);
}
