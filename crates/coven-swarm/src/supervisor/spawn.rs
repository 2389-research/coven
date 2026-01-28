// ABOUTME: Spawns and manages workspace agent child processes.
// ABOUTME: Tracks process state and handles restarts.

use super::tui::TuiEvent;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub struct AgentProcess {
    pub workspace: String,
    pub dispatch_mode: bool,
    child: Option<Child>,
    config_path: PathBuf,
    pid: Option<u32>,
}

impl AgentProcess {
    pub fn new(workspace: String, config_path: PathBuf, dispatch_mode: bool) -> Self {
        Self {
            workspace,
            dispatch_mode,
            child: None,
            config_path,
            pid: None,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Spawn with optional TUI event sender (headless mode uses None)
    pub async fn spawn_with_tui(&mut self, tui_tx: Option<mpsc::Sender<TuiEvent>>) -> Result<()> {
        let exe = std::env::current_exe()?;

        // Detect if we're running as unified `coven` CLI or standalone `coven-swarm`
        let is_unified_cli = exe
            .file_name()
            .map(|n| n.to_string_lossy())
            .map(|n| n == "coven" || n.starts_with("coven."))
            .unwrap_or(false);

        let mut cmd = Command::new(&exe);

        // For unified CLI: `coven swarm agent --workspace ...`
        // For standalone:  `coven-swarm agent --workspace ...`
        if is_unified_cli {
            cmd.arg("swarm");
        }
        cmd.arg("agent")
            .arg("--workspace")
            .arg(&self.workspace)
            .arg("--config")
            .arg(&self.config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if self.dispatch_mode {
            cmd.arg("--dispatch-mode");
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn agent for {}", self.workspace))?;

        self.pid = child.id();

        if tui_tx.is_none() {
            tracing::info!(workspace = %self.workspace, pid = ?child.id(), "Spawned agent");
        }

        // Spawn tasks to forward stdout/stderr with workspace prefix
        let workspace_name = self.workspace.clone();
        if let Some(stdout) = child.stdout.take() {
            let ws = workspace_name.clone();
            let tx = tui_tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ref tx) = tx {
                        let _ = tx.send(TuiEvent::AgentLog {
                            workspace: ws.clone(),
                            line: line.clone(),
                        }).await;
                    } else {
                        eprintln!("[{}] {}", ws, line);
                    }
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let ws = workspace_name;
            let tx = tui_tx;
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ref tx) = tx {
                        let _ = tx.send(TuiEvent::AgentLog {
                            workspace: ws.clone(),
                            line: line.clone(),
                        }).await;
                    } else {
                        eprintln!("[{}] {}", ws, line);
                    }
                }
            });
        }

        self.child = Some(child);
        Ok(())
    }

    /// Spawn in headless mode (backward compatible)
    #[allow(dead_code)]
    pub async fn spawn(&mut self) -> Result<()> {
        self.spawn_with_tui(None).await
    }

    pub fn is_running(&self) -> bool {
        // If we have a pid, assume the process is running
        // Accurate check would require try_wait but that requires &mut
        self.pid.is_some() && self.child.is_some()
    }

    #[allow(dead_code)]
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        match &mut self.child {
            Some(child) => Ok(child.wait().await?),
            None => anyhow::bail!("No child process"),
        }
    }

    pub async fn kill(&mut self) -> Result<()> {
        if let Some(child) = &mut self.child {
            child.kill().await?;
        }
        Ok(())
    }
}
