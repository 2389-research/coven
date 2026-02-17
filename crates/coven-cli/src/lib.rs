// ABOUTME: CLI library components for the unified coven CLI.
// ABOUTME: Shared utilities and command implementations for coven subcommands.

//! # coven-cli
//!
//! Unified command-line interface for coven agent orchestration.
//!
//! This crate provides the `coven` binary which consolidates all coven
//! commands under a single entry point:
//!
//! ```text
//! coven
//! ├── init                          # First-time setup wizard
//! ├── swarm
//! │   ├── start                     # Start supervisor daemon
//! │   ├── stop                      # Stop supervisor
//! │   └── status                    # Show running agents
//! ├── agent
//! │   ├── run                       # Run individual agent
//! │   └── new                       # Create agent config
//! ├── chat                          # Open TUI
//! ├── human                         # Act as human agent
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
//! coven init
//!
//! # Start the swarm supervisor
//! coven swarm start
//!
//! # Run a single agent
//! coven agent run --name my-agent
//!
//! # Open the chat TUI
//! coven chat
//!
//! # List available packs
//! coven pack list
//! ```

/// Version of the coven CLI
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
