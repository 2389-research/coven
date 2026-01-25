// ABOUTME: Working directory-aware wrapper tools for mux backend.
// ABOUTME: Resolves relative paths against the channel's working directory.

use async_trait::async_trait;
use mux::tool::{Tool, ToolResult};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

/// Helper to resolve a path against the working directory.
/// Returns None if the path would escape the working directory.
fn resolve_path(working_dir: &Path, path: &str) -> Option<PathBuf> {
    let p = Path::new(path);

    // Resolve the path relative to working_dir
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    };

    // Canonicalize to resolve .. and symlinks, then verify it's within working_dir
    // If working_dir isn't canonicalized yet, do that too
    let canonical_working = working_dir.canonicalize().ok()?;

    // For paths that don't exist yet (write operations), canonicalize the parent
    if resolved.exists() {
        let canonical_resolved = resolved.canonicalize().ok()?;
        if canonical_resolved.starts_with(&canonical_working) {
            Some(canonical_resolved)
        } else {
            None // Path escapes working directory
        }
    } else {
        // For new files, check that the parent is within working_dir
        // and construct the full path
        if let Some(parent) = resolved.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().ok()?;
                if canonical_parent.starts_with(&canonical_working) {
                    // Reconstruct path with canonical parent + filename
                    resolved
                        .file_name()
                        .map(|filename| canonical_parent.join(filename))
                } else {
                    None
                }
            } else {
                // Parent doesn't exist - could be creating nested dirs
                // Be conservative and reject if any path component is ParentDir (..)
                let has_parent_dir = Path::new(path)
                    .components()
                    .any(|c| matches!(c, Component::ParentDir));
                if has_parent_dir {
                    None
                } else if p.is_absolute()
                    && !(resolved.starts_with(working_dir)
                        || resolved.starts_with(&canonical_working))
                {
                    None
                } else {
                    Some(resolved)
                }
            }
        } else {
            None
        }
    }
}

/// ReadFileTool with working directory support.
pub struct WdReadFileTool {
    working_dir: PathBuf,
}

impl WdReadFileTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Relative paths are resolved from your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to read (relative to working directory or absolute)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            path: String,
        }
        let params: Params = serde_json::from_value(params)?;
        let Some(resolved) = resolve_path(&self.working_dir, &params.path) else {
            return Ok(ToolResult::error(format!(
                "Path '{}' is outside the working directory",
                params.path
            )));
        };

        match std::fs::read_to_string(&resolved) {
            Ok(content) => Ok(ToolResult::text(content)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to read file '{}': {}",
                resolved.display(),
                e
            ))),
        }
    }
}

/// WriteFileTool with working directory support.
pub struct WdWriteFileTool {
    working_dir: PathBuf,
}

impl WdWriteFileTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdWriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed. Relative paths are resolved from your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to write (relative to working directory or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            path: String,
            content: String,
        }
        let params: Params = serde_json::from_value(params)?;
        let Some(resolved) = resolve_path(&self.working_dir, &params.path) else {
            return Ok(ToolResult::error(format!(
                "Path '{}' is outside the working directory",
                params.path
            )));
        };

        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        match std::fs::write(&resolved, &params.content) {
            Ok(()) => Ok(ToolResult::text(format!(
                "Successfully wrote {} bytes to {}",
                params.content.len(),
                resolved.display()
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to write file '{}': {}",
                resolved.display(),
                e
            ))),
        }
    }
}

/// ListFilesTool with working directory support.
pub struct WdListFilesTool {
    working_dir: PathBuf,
}

impl WdListFilesTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files in a directory matching a glob pattern. Defaults to your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory to list (default: working directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to match (default: *)"
                }
            }
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize, Default)]
        struct Params {
            path: Option<String>,
            glob: Option<String>,
        }
        let params: Params = serde_json::from_value(params).unwrap_or_default();

        // Use working_dir as default if no path specified
        let base_path = match &params.path {
            Some(p) => {
                let Some(resolved) = resolve_path(&self.working_dir, p) else {
                    return Ok(ToolResult::error(format!(
                        "Path '{}' is outside the working directory",
                        p
                    )));
                };
                resolved
            }
            None => self.working_dir.clone(),
        };
        let glob_pattern = params.glob.unwrap_or_else(|| "*".to_string());
        let full_pattern = format!("{}/{}", base_path.display(), glob_pattern);

        let mut files = Vec::new();
        for path in glob::glob(&full_pattern)
            .unwrap_or_else(|_| glob::glob("").unwrap())
            .flatten()
        {
            // Show paths relative to working_dir for cleaner output
            let display_path = path
                .strip_prefix(&self.working_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            let prefix = if path.is_dir() { "[dir] " } else { "" };
            files.push(format!("{}{}", prefix, display_path));
        }

        if files.is_empty() {
            Ok(ToolResult::text("No files found"))
        } else {
            Ok(ToolResult::text(files.join("\n")))
        }
    }
}

/// SearchTool with working directory support.
pub struct WdSearchTool {
    working_dir: PathBuf,
}

impl WdSearchTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdSearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for a pattern in files. Supports glob patterns for file matching and regex for content matching. Defaults to your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in (default: working directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern for files to search (default: **/*)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            pattern: String,
            path: Option<String>,
            glob: Option<String>,
        }
        let params: Params = serde_json::from_value(params)?;

        let base_path = match &params.path {
            Some(p) => {
                let Some(resolved) = resolve_path(&self.working_dir, p) else {
                    return Ok(ToolResult::error(format!(
                        "Path '{}' is outside the working directory",
                        p
                    )));
                };
                resolved
            }
            None => self.working_dir.clone(),
        };
        let glob_pattern = params.glob.unwrap_or_else(|| "**/*".to_string());
        let full_pattern = format!("{}/{}", base_path.display(), glob_pattern);

        let mut results = Vec::new();
        let regex = match regex::Regex::new(&params.pattern) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Invalid regex: {}", e))),
        };

        for path in glob::glob(&full_pattern)
            .unwrap_or_else(|_| glob::glob("").unwrap())
            .flatten()
        {
            if path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (line_num, line) in content.lines().enumerate() {
                        if regex.is_match(line) {
                            // Show paths relative to working_dir
                            let display_path = path
                                .strip_prefix(&self.working_dir)
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| path.display().to_string());
                            results.push(format!(
                                "{}:{}: {}",
                                display_path,
                                line_num + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(ToolResult::text("No matches found"))
        } else {
            Ok(ToolResult::text(format!(
                "Found {} matches:\n{}",
                results.len(),
                results.join("\n")
            )))
        }
    }
}

/// EditTool with working directory support.
/// Performs precise string replacement in files.
pub struct WdEditTool {
    working_dir: PathBuf,
}

impl WdEditTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdEditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string with new content. More precise than rewriting the entire file. Relative paths are resolved from your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to edit (relative to working directory or absolute)"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find and replace (must match exactly, including whitespace)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The string to replace it with"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            path: String,
            old_string: String,
            new_string: String,
        }
        let params: Params = serde_json::from_value(params)?;
        let Some(resolved) = resolve_path(&self.working_dir, &params.path) else {
            return Ok(ToolResult::error(format!(
                "Path '{}' is outside the working directory",
                params.path
            )));
        };

        // Read the file
        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to read file '{}': {}",
                    resolved.display(),
                    e
                )));
            }
        };

        // Check if old_string exists
        if !content.contains(&params.old_string) {
            return Ok(ToolResult::error(format!(
                "The string to replace was not found in '{}'. Make sure the old_string matches exactly, including whitespace and indentation.",
                resolved.display()
            )));
        }

        // Check for uniqueness (should only match once for safety)
        let matches: Vec<_> = content.match_indices(&params.old_string).collect();
        if matches.len() > 1 {
            return Ok(ToolResult::error(format!(
                "The string to replace was found {} times in '{}'. Please provide a more unique string that matches exactly once.",
                matches.len(),
                resolved.display()
            )));
        }

        // Perform the replacement
        let new_content = content.replacen(&params.old_string, &params.new_string, 1);

        // Write the file
        match std::fs::write(&resolved, &new_content) {
            Ok(()) => Ok(ToolResult::text(format!(
                "Successfully edited {}",
                resolved.display()
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to write file '{}': {}",
                resolved.display(),
                e
            ))),
        }
    }
}

/// BashTool with working directory default.
pub struct WdBashTool {
    working_dir: PathBuf,
}

impl WdBashTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdBashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command. Commands run in your working directory by default."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Override the working directory for this command (default: your working directory)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            command: String,
            working_dir: Option<String>,
        }
        let params: Params = serde_json::from_value(params)?;

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&params.command);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Use provided working_dir or default to our working_dir
        let cwd = match &params.working_dir {
            Some(dir) => {
                let Some(resolved) = resolve_path(&self.working_dir, dir) else {
                    return Ok(ToolResult::error(format!(
                        "Working directory '{}' is outside the allowed directory",
                        dir
                    )));
                };
                resolved
            }
            None => self.working_dir.clone(),
        };
        cmd.current_dir(&cwd);

        // 5 minute timeout to prevent runaway commands
        let output =
            match tokio::time::timeout(std::time::Duration::from_secs(300), cmd.output()).await {
                Ok(result) => result?,
                Err(_) => {
                    return Ok(ToolResult::error(
                        "Command timed out after 5 minutes".to_string(),
                    ));
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if output.status.success() {
            if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{}\n\nstderr:\n{}", stdout, stderr)
            }
        } else {
            format!(
                "Command failed with exit code {}\n\nstdout:\n{}\n\nstderr:\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            )
        };

        if output.status.success() {
            Ok(ToolResult::text(result))
        } else {
            Ok(ToolResult::error(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_relative() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "Hello!").unwrap();

        let tool = WdReadFileTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"path": "test.txt"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.content, "Hello!");
    }

    #[tokio::test]
    async fn test_write_file_relative() {
        let dir = TempDir::new().unwrap();

        let tool = WdWriteFileTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"path": "test.txt", "content": "Hello!"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("test.txt")).unwrap(),
            "Hello!"
        );
    }

    #[tokio::test]
    async fn test_bash_uses_working_dir() {
        let dir = TempDir::new().unwrap();

        let tool = WdBashTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"command": "pwd"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        // The output should contain the temp dir path
        assert!(
            result.content.contains(dir.path().to_str().unwrap())
                || result.content.contains("private")
        ); // macOS /tmp -> /private/tmp
    }

    #[test]
    fn test_resolve_path_allows_double_dots_in_filename() {
        // Files like "foo..bar" should be allowed - they don't contain path traversal
        let dir = TempDir::new().unwrap();
        let result = resolve_path(dir.path(), "foo..bar");
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("foo..bar"));
    }

    #[test]
    fn test_resolve_path_rejects_parent_traversal() {
        // "../escape" should be rejected as it attempts to escape
        let dir = TempDir::new().unwrap();
        let result = resolve_path(dir.path(), "../escape");
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_path_rejects_embedded_parent_traversal() {
        // "a/../b" should be rejected when parent doesn't exist
        let dir = TempDir::new().unwrap();
        let result = resolve_path(dir.path(), "a/../b");
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_path_rejects_absolute_outside_workdir() {
        let dir = TempDir::new().unwrap();
        let outside = std::env::temp_dir().join("fold-agent-abs-outside");
        let result = resolve_path(dir.path(), outside.to_str().unwrap());
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_path_allows_absolute_inside_workdir() {
        let dir = TempDir::new().unwrap();
        let inside = dir.path().join("newdir/file.txt");
        let result = resolve_path(dir.path(), inside.to_str().unwrap());
        assert!(result.is_some());
    }

    #[test]
    fn test_resolve_path_allows_simple_nested_path() {
        // "a/b/c" should be allowed for non-existent nested paths
        let dir = TempDir::new().unwrap();
        let result = resolve_path(dir.path(), "a/b/c");
        assert!(result.is_some());
    }
}
