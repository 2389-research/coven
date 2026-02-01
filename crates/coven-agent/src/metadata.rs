// ABOUTME: Gathers environment metadata for agent registration.
// ABOUTME: Collects git info, hostname, OS at startup.

use std::path::Path;
use std::process::Command;

/// Default capabilities for agents when not specified in config.
/// - "base": Access to gateway builtin tools (log, todo, bbs)
/// - "chat": Basic chat/messaging capability
pub fn default_capabilities() -> Vec<String> {
    vec!["base".to_string(), "chat".to_string()]
}

/// Git repository state
#[derive(Debug, Clone, Default)]
pub struct GitInfo {
    pub branch: String,
    pub commit: String,
    pub dirty: bool,
    pub remote: String,
    pub ahead: i32,
    pub behind: i32,
}

/// Environment metadata sent during registration
#[derive(Debug, Clone)]
pub struct AgentMetadata {
    pub working_directory: String,
    pub git: Option<GitInfo>,
    pub hostname: String,
    pub os: String,
    /// Workspace tags for filtering (set from config)
    pub workspaces: Vec<String>,
    /// Backend type: "mux" or "cli" (set by caller)
    pub backend: String,
    /// Capabilities this agent supports (set from config, defaults to ["base", "chat"])
    pub capabilities: Vec<String>,
}

impl GitInfo {
    /// Gather git info from the given directory. Returns None if not a git repo.
    pub fn gather(working_dir: &Path) -> Option<Self> {
        // Check if this is a git repo
        let status = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(working_dir)
            .output()
            .ok()?;

        if !status.status.success() {
            return None;
        }

        let branch =
            run_git(working_dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

        let commit = run_git(working_dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();

        let dirty = run_git(working_dir, &["status", "--porcelain"])
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        // Get remote tracking branch (may not exist)
        let remote =
            run_git(working_dir, &["rev-parse", "--abbrev-ref", "@{u}"]).unwrap_or_default();

        // Get ahead/behind counts (only if we have a remote)
        let (ahead, behind) = if !remote.is_empty() {
            parse_ahead_behind(working_dir)
        } else {
            (0, 0)
        };

        Some(GitInfo {
            branch,
            commit,
            dirty,
            remote,
            ahead,
            behind,
        })
    }
}

fn run_git(working_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(working_dir)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn parse_ahead_behind(working_dir: &Path) -> (i32, i32) {
    // git rev-list --left-right --count @{u}...HEAD
    // Output: "3\t5" meaning 3 behind, 5 ahead
    let output = run_git(
        working_dir,
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
    );

    match output {
        Some(s) => {
            let parts: Vec<&str> = s.split('\t').collect();
            if parts.len() == 2 {
                let behind = parts[0].parse().unwrap_or(0);
                let ahead = parts[1].parse().unwrap_or(0);
                (ahead, behind)
            } else {
                (0, 0)
            }
        }
        None => (0, 0),
    }
}

impl AgentMetadata {
    /// Gather all metadata for the given working directory
    pub fn gather(working_dir: &Path) -> Self {
        let working_directory = working_dir
            .canonicalize()
            .unwrap_or_else(|_| working_dir.to_path_buf())
            .display()
            .to_string();

        let git = GitInfo::gather(working_dir);

        let hostname = get_hostname();

        let os = std::env::consts::OS.to_string();

        AgentMetadata {
            working_directory,
            git,
            hostname,
            os,
            workspaces: Vec::new(),   // Set by caller from config
            backend: String::new(),   // Set by caller
            capabilities: Vec::new(), // Set by caller from config
        }
    }
}

fn get_hostname() -> String {
    // Try hostname command first (works on macOS and Linux)
    if let Ok(output) = Command::new("hostname").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }

    // Fallback to environment variable
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

// Conversion to protobuf types
impl From<GitInfo> for coven_proto::GitInfo {
    fn from(info: GitInfo) -> Self {
        coven_proto::GitInfo {
            branch: info.branch,
            commit: info.commit,
            dirty: info.dirty,
            remote: info.remote,
            ahead: info.ahead,
            behind: info.behind,
        }
    }
}

impl From<AgentMetadata> for coven_proto::AgentMetadata {
    fn from(meta: AgentMetadata) -> Self {
        coven_proto::AgentMetadata {
            working_directory: meta.working_directory,
            git: meta.git.map(Into::into),
            hostname: meta.hostname,
            os: meta.os,
            workspaces: meta.workspaces,
            backend: meta.backend,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_metadata_gather_in_git_repo() {
        // Use the project root which is a git repo
        let working_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        let metadata = AgentMetadata::gather(&working_dir);

        assert!(!metadata.working_directory.is_empty());
        assert!(!metadata.hostname.is_empty());
        assert!(!metadata.os.is_empty());

        // Should have git info since we're in a git repo
        let git = metadata.git.expect("should have git info");
        assert!(!git.branch.is_empty());
        assert!(!git.commit.is_empty());
    }

    #[test]
    fn test_metadata_gather_not_git_repo() {
        // Use /tmp which is not a git repo
        let metadata = AgentMetadata::gather(Path::new("/tmp"));

        assert!(!metadata.working_directory.is_empty());
        assert!(!metadata.hostname.is_empty());
        assert!(metadata.git.is_none());
    }

    #[test]
    fn test_git_info_branch_and_commit() {
        let working_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        let git = GitInfo::gather(&working_dir).expect("should be in git repo");

        // Commit should be a short hash (7-8 chars typically)
        assert!(git.commit.len() >= 7);
        assert!(git.commit.len() <= 12);

        // Branch should be non-empty
        assert!(!git.branch.is_empty());
    }

    #[test]
    fn test_os_is_known_value() {
        let metadata = AgentMetadata::gather(Path::new("/tmp"));

        // Should be one of the known OS values
        assert!(
            ["macos", "linux", "windows", "freebsd", "openbsd", "netbsd"]
                .contains(&metadata.os.as_str()),
            "unexpected OS: {}",
            metadata.os
        );
    }

    #[test]
    fn test_backend_defaults_to_empty() {
        // Backend should default to empty string (set by caller)
        let metadata = AgentMetadata::gather(Path::new("/tmp"));
        assert!(
            metadata.backend.is_empty(),
            "backend should default to empty"
        );
    }

    #[test]
    fn test_metadata_to_proto_includes_backend() {
        let mut metadata = AgentMetadata::gather(Path::new("/tmp"));
        metadata.backend = "mux".to_string();

        let proto_meta: coven_proto::AgentMetadata = metadata.into();
        assert_eq!(proto_meta.backend, "mux");
    }
}
