// ABOUTME: Entry point for coven-chat TUI
// ABOUTME: Handles CLI args, config loading, and TUI launch

use clap::Parser;

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

fn main() {
    let _args = Args::parse();
    println!("coven-chat v2 skeleton");
}
