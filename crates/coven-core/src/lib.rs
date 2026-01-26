// ABOUTME: Core library for coven - types, router, backend, storage
// ABOUTME: Shared between coven-agent and coven-server

pub mod backend;
pub mod config;
pub mod files;
pub mod mcp_http;
pub mod router;
pub mod store;
pub mod types;

pub use backend::{BackendEvent, ToolStateKind};
pub use config::Config;
pub use files::SessionFiles;
pub use router::Coven;
pub use store::ThreadStore;
pub use types::{FileAttachment, IncomingMessage, OutgoingEvent, Thread};
