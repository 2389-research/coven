// ABOUTME: Discovers existing workspace directories.
// ABOUTME: Scans working directory for subdirectories to spawn agents for.

use anyhow::Result;
use std::path::Path;

/// Discover workspace directories (excluding hidden directories)
pub fn discover_workspaces(working_dir: &Path) -> Result<Vec<String>> {
    let mut workspaces = Vec::new();

    for entry in std::fs::read_dir(working_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        // Skip hidden directories
        if name.starts_with('.') {
            continue;
        }

        workspaces.push(name.to_string());
    }

    Ok(workspaces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_workspaces() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("research")).unwrap();
        std::fs::create_dir(tmp.path().join("weather")).unwrap();
        std::fs::create_dir(tmp.path().join(".hidden")).unwrap();

        let workspaces = discover_workspaces(tmp.path()).unwrap();
        assert_eq!(workspaces.len(), 2);
        assert!(workspaces.contains(&"research".to_string()));
        assert!(workspaces.contains(&"weather".to_string()));
    }

    #[test]
    fn test_discover_workspaces_skips_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("workspace1")).unwrap();
        std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

        let workspaces = discover_workspaces(tmp.path()).unwrap();
        assert_eq!(workspaces.len(), 1);
        assert!(workspaces.contains(&"workspace1".to_string()));
    }

    #[test]
    fn test_discover_workspaces_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let workspaces = discover_workspaces(tmp.path()).unwrap();
        assert!(workspaces.is_empty());
    }
}
