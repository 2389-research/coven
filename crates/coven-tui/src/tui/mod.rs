// ABOUTME: Terminal setup and event stream handling.
// ABOUTME: Manages crossterm and ratatui terminal lifecycle.

#![allow(dead_code)]

pub mod event;
pub mod frame;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, Stdout};
use std::time::Duration;

use crate::error::Result;
use event::EventStream;

pub type Terminal = ratatui::Terminal<CrosstermBackend<Stdout>>;

pub struct Tui {
    terminal: Terminal,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let terminal = Self::setup_terminal()?;
        Ok(Self { terminal })
    }

    fn setup_terminal() -> Result<Terminal> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(terminal)
    }

    pub fn restore(&mut self) -> Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }

    pub fn event_stream(&self) -> EventStream {
        EventStream::new(Duration::from_millis(80))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
