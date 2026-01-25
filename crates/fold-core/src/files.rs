// ABOUTME: Session file management for ephemeral file storage
// ABOUTME: Handles temp directories for incoming/outgoing files per session

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Manages ephemeral file storage for sessions
pub struct SessionFiles {
    base_dir: PathBuf,
}

impl SessionFiles {
    /// Create a new SessionFiles manager
    pub fn new() -> Result<Self> {
        let base_dir = std::env::temp_dir().join("fold-sessions");
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Get the base directory for a session
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(sanitize_session_id(session_id))
    }

    /// Get the incoming files directory for a session
    pub fn incoming_dir(&self, session_id: &str) -> Result<PathBuf> {
        let dir = self.session_dir(session_id).join("incoming");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Get the outgoing files directory for a session
    pub fn outgoing_dir(&self, session_id: &str) -> Result<PathBuf> {
        let dir = self.session_dir(session_id).join("outgoing");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Cleanup stale sessions older than max_age
    pub fn cleanup_stale(&self, max_age: Duration) -> Result<usize> {
        let mut cleaned = 0;
        if !self.base_dir.exists() {
            return Ok(0);
        }

        for entry in fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;

            let modified = metadata.modified()?;
            if let Ok(age) = modified.elapsed() {
                if age > max_age {
                    if let Err(e) = fs::remove_dir_all(entry.path()) {
                        eprintln!(
                            "Warning: Failed to cleanup session dir {:?}: {}",
                            entry.path(),
                            e
                        );
                    } else {
                        cleaned += 1;
                    }
                }
            }
        }
        Ok(cleaned)
    }

    /// Delete a specific session's files
    pub fn cleanup_session(&self, session_id: &str) -> Result<()> {
        let dir = self.session_dir(session_id);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Get the base directory path
    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }
}

impl Default for SessionFiles {
    fn default() -> Self {
        Self::new().expect("Failed to create SessionFiles")
    }
}

// Note: No Drop impl - cleanup is handled explicitly via cleanup_stale() and cleanup_session()
// This avoids the problem of multiple SessionFiles instances deleting each other's files

/// Sanitize session ID for use as directory name (prevents path traversal)
fn sanitize_session_id(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Sanitize a filename to prevent path traversal attacks
/// Removes path separators and parent directory references
pub fn sanitize_filename(filename: &str) -> String {
    // Normalize backslashes to forward slashes (handles Windows paths on Unix)
    let normalized = filename.replace('\\', "/");

    // Get just the filename component (removes any path)
    let name = std::path::Path::new(&normalized)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed");

    // Replace any remaining dangerous characters (null bytes)
    name.chars()
        .map(|c| match c {
            '\0' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_session_id() {
        assert_eq!(sanitize_session_id("abc-123"), "abc-123");
        assert_eq!(sanitize_session_id("abc:def"), "abc_def");
        assert_eq!(sanitize_session_id("a/b/c"), "a_b_c");
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("../../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\windows\\system32"), "system32");
        assert_eq!(sanitize_filename("file\0name.txt"), "file_name.txt");
    }

    #[test]
    fn test_session_dirs() -> Result<()> {
        let files = SessionFiles::new()?;
        let incoming = files.incoming_dir("test-session")?;
        let outgoing = files.outgoing_dir("test-session")?;

        assert!(incoming.exists());
        assert!(outgoing.exists());
        assert!(incoming.ends_with("incoming"));
        assert!(outgoing.ends_with("outgoing"));

        files.cleanup_session("test-session")?;
        Ok(())
    }
}
