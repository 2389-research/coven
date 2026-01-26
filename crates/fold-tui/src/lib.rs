// ABOUTME: Library interface for fold-tui.
// ABOUTME: Exposes the TUI runner for use by the unified fold CLI.

mod app;
mod app_event;
pub mod cli;
mod client_bridge;
pub mod error;
pub mod state;
mod theme;
mod tui;
mod widgets;

use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};
use rand::seq::SliceRandom;
use state::config::Config;
use std::io::Write;

const GOODBYES: &[&str] = &[
    "See you soon!",
    "Until next time!",
    "Goodbye for now!",
    "Catch you later!",
    "Stay orchestrated!",
    "Keep automating!",
    "The agents await your return.",
    "Till we meet again!",
    "Happy building!",
    "Go make something great!",
];

const TAGLINES: &[&str] = &[
    "The future of work is orchestrated.",
    "Agents at your service.",
    "Go forth and automate.",
    "The hive mind rests.",
    "Async dreams await.",
    "May your pipelines flow.",
    "Orchestration complete.",
    "The swarm sleeps.",
];

fn print_exit_message() {
    let mut rng = rand::thread_rng();
    let goodbye = GOODBYES.choose(&mut rng).unwrap_or(&"See you soon!");
    let tagline = TAGLINES
        .choose(&mut rng)
        .unwrap_or(&"The future of work is orchestrated.");

    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout);
    let _ = write!(stdout, "  {}", SetForegroundColor(Color::Cyan));
    let _ = write!(stdout, "{}", goodbye);
    let _ = writeln!(stdout, "{}", ResetColor);
    let _ = writeln!(stdout);
    let _ = write!(stdout, "  {}", SetForegroundColor(Color::DarkGrey));
    let _ = write!(stdout, "─═══─");
    let _ = write!(stdout, "{}", ResetColor);
    let _ = write!(stdout, " {}", SetForegroundColor(Color::Cyan));
    let _ = write!(stdout, "⬡");
    let _ = write!(stdout, "{}", ResetColor);
    let _ = write!(stdout, "  {}", SetForegroundColor(Color::Cyan));
    let _ = write!(stdout, "{}", SetAttribute(Attribute::Bold));
    let _ = write!(stdout, "2389.ai");
    let _ = write!(stdout, "{}", SetAttribute(Attribute::Reset));
    let _ = write!(stdout, " {}", SetForegroundColor(Color::Cyan));
    let _ = write!(stdout, "⬡");
    let _ = write!(stdout, "{}", ResetColor);
    let _ = write!(stdout, "  {}", SetForegroundColor(Color::DarkGrey));
    let _ = write!(stdout, "─═══─");
    let _ = writeln!(stdout, "{}", ResetColor);
    let _ = write!(stdout, "  {}", SetForegroundColor(Color::DarkGrey));
    let _ = write!(stdout, "{}", tagline);
    let _ = writeln!(stdout, "{}", ResetColor);
    let _ = writeln!(stdout);
}

/// Run the TUI chat interface.
///
/// This function takes over the terminal and runs the full TUI application.
/// It will block until the user exits the TUI.
///
/// # Arguments
/// * `gateway` - Optional gateway URL override (e.g., "http://localhost:5000")
/// * `theme` - Optional theme name override
///
/// # Errors
/// Returns an error if the TUI fails to initialize or run.
pub async fn run_chat(gateway: Option<String>, theme: Option<String>) -> error::Result<()> {
    let config = Config::load(gateway.as_deref(), theme.as_deref())?;
    tracing::debug!(?config, "Loaded configuration");

    let mut tui = tui::Tui::new()?;
    let mut app = app::App::new(&config)?;
    app.run(&mut tui).await?;

    print_exit_message();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_message_arrays_are_non_empty() {
        assert!(!GOODBYES.is_empty(), "GOODBYES array must not be empty");
        assert!(!TAGLINES.is_empty(), "TAGLINES array must not be empty");
    }
}
