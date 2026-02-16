// ABOUTME: Entry point for coven-chat TUI binary.
// ABOUTME: Parses CLI args and delegates to the library's run module.

use anyhow::{Context, Result};
use clap::Parser;
use coven_link::config::CovenConfig;

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

fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands (these don't need the TUI)
    match args.command {
        Some(Command::Send { message, print: _ }) => {
            coven_log::init_file("tui");

            let config = CovenConfig::load().context(
                "No coven config found. Run 'coven link' first to set up gateway connection.",
            )?;

            let gw_url = if config.gateway.starts_with("http://")
                || config.gateway.starts_with("https://")
            {
                config.gateway.clone()
            } else {
                format!("http://{}", config.gateway)
            };

            let key_path = CovenConfig::key_path()?;
            coven_tui_v2::cli::send::run(&gw_url, &key_path, &message, args.agent.as_deref())?;
            Ok(())
        }
        None => {
            // Run interactive TUI via the library entry point
            coven_tui_v2::run::run(args.agent)
        }
    }
}
