// ABOUTME: Pluggable Claude backend abstraction for coven-swarm.
// ABOUTME: Re-exports coven-core's Backend trait and adds ACP + dispatch_tools.

pub mod handle;

#[cfg(feature = "acp")]
pub mod acp;

pub mod dispatch_tools;

pub use coven_core::backend::{Backend, BackendEvent, ToolStateKind};
pub use handle::BackendHandle;
