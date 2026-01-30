// ABOUTME: Send+Sync wrapper for coven-core backends.
// ABOUTME: Provides thread-safe access to Backend::send().

use crate::{Backend, BackendEvent};
use anyhow::Result;
use futures::stream::BoxStream;
use std::sync::Arc;

/// Send+Sync handle to a backend
#[derive(Clone)]
pub struct BackendHandle {
    backend: Arc<dyn Backend>,
}

impl BackendHandle {
    pub fn new<B: Backend + 'static>(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
        }
    }

    /// Create from an existing Arc'd backend (allows keeping a reference to concrete type)
    pub fn new_from_arc<B: Backend + 'static>(backend: Arc<B>) -> Self {
        Self { backend }
    }

    pub fn name(&self) -> &'static str {
        self.backend.name()
    }

    pub async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>> {
        self.backend.send(session_id, message, is_new_session).await
    }
}

impl std::fmt::Debug for BackendHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendHandle")
            .field("name", &self.backend.name())
            .finish()
    }
}
