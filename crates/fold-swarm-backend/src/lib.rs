// ABOUTME: Pluggable Claude backend abstraction for fold-swarm.
// ABOUTME: Re-exports fold-core's Backend trait and adds ACP + dispatch_tools.

pub mod handle;

#[cfg(feature = "acp")]
pub mod acp;

pub mod dispatch_tools;

pub use fold_core::backend::{Backend, BackendEvent, ToolStateKind};
pub use handle::BackendHandle;
