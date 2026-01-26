// ABOUTME: Entry point for the coven-tui application.
// ABOUTME: Parses CLI args and launches TUI or runs subcommands.

mod app;
mod app_event;
mod cli;
mod client_bridge;
mod error;
mod state;
mod theme;
mod tui;
mod widgets;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
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

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    // Load config with CLI overrides
    let config = match Config::load(cli.gateway.as_deref(), cli.theme.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    };

    tracing::debug!(?config, "Loaded configuration");

    match cli.command {
        Some(Command::Version) => {
            println!("coven-chat {}", env!("CARGO_PKG_VERSION"));
        }
        Some(Command::Config { action }) => {
            handle_config_command(action, &config);
        }
        Some(Command::Doctor) => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create async runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(cli::doctor::run(&config)) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Agents) => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create async runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(cli::agents::run(&config)) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Send { agent, message }) => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create async runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(cli::send::run(&config, &agent, &message)) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Themes { action }) => {
            handle_themes_command(action, config);
        }
        Some(Command::Setup) => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create async runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(cli::setup::run()) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Completion { shell }) => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "coven-chat", &mut std::io::stdout());
        }
        None => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create async runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(run_tui(&config)) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            print_exit_message();
        }
    }
}

async fn run_tui(config: &Config) -> error::Result<()> {
    let mut tui = tui::Tui::new()?;
    let mut app = app::App::new(config)?;
    app.run(&mut tui).await?;
    Ok(())
}

fn handle_config_command(action: Option<cli::ConfigAction>, config: &Config) {
    use cli::ConfigAction;

    match action {
        Some(ConfigAction::Path) => match Config::config_path() {
            Ok(path) => println!("{}", path.display()),
            Err(e) => eprintln!("Error: {}", e),
        },
        Some(ConfigAction::Edit) => match Config::config_path() {
            Ok(path) => {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                let _ = std::process::Command::new(editor).arg(&path).status();
            }
            Err(e) => eprintln!("Error: {}", e),
        },
        Some(ConfigAction::Set { pair }) => {
            println!("Set config: {} (not yet implemented)", pair);
        }
        None => {
            // Print current config
            println!("[gateway]");
            println!("host = \"{}\"", config.gateway.host);
            println!("port = {}", config.gateway.port);
            println!();
            println!("[appearance]");
            println!("theme = \"{}\"", config.appearance.theme);
        }
    }
}

fn handle_themes_command(action: Option<cli::ThemeAction>, mut config: Config) {
    use cli::ThemeAction;

    match action {
        Some(ThemeAction::List) | None => {
            cli::themes::list_themes(&config);
        }
        Some(ThemeAction::Set { name }) => {
            if let Err(e) = cli::themes::set_theme(&mut config, &name) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
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
