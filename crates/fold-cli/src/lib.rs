// ABOUTME: CLI library components for the unified fold CLI.
// ABOUTME: Shared utilities and command implementations for fold subcommands.

//! # fold-cli
//!
//! Unified command-line interface for fold agent orchestration.
//!
//! This crate provides the `fold` binary which consolidates all fold
//! commands under a single entry point:
//!
//! ```text
//! fold
//! ├── init                          # First-time setup wizard
//! ├── swarm
//! │   ├── start                     # Start supervisor daemon
//! │   ├── stop                      # Stop supervisor
//! │   └── status                    # Show running agents
//! ├── agent
//! │   ├── run                       # Run individual agent
//! │   └── new                       # Create agent config
//! ├── chat                          # Open TUI
//! ├── pack
//! │   ├── list                      # List available packs
//! │   ├── install <pack>            # Install a pack
//! │   └── run <pack>                # Run pack directly
//! └── version                       # Show version info
//! ```
//!
//! ## Usage
//!
//! ```bash
//! # First-time setup
//! fold init
//!
//! # Start the swarm supervisor
//! fold swarm start
//!
//! # Run a single agent
//! fold agent run --name my-agent
//!
//! # Open the chat TUI
//! fold chat
//!
//! # List available packs
//! fold pack list
//! ```

/// Version of the fold CLI
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
