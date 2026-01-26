// ABOUTME: Unix socket API for dispatch agent to manage swarm.
// ABOUTME: Provides endpoints to create/delete/list workspaces.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    #[serde(rename = "list")]
    List,
    #[serde(rename = "create")]
    Create { name: String },
    #[serde(rename = "delete")]
    Delete { name: String },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "stop")]
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    pub prefix: String,
    pub agents: Vec<AgentStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub workspace: String,
    pub pid: Option<u32>,
    pub running: bool,
}

/// Commands sent to supervisor from socket handler
pub enum SocketCommand {
    List {
        reply: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    Create {
        name: String,
        reply: tokio::sync::oneshot::Sender<Result<String>>,
    },
    Delete {
        name: String,
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Status {
        reply: tokio::sync::oneshot::Sender<StatusInfo>,
    },
    Stop {
        reply: tokio::sync::oneshot::Sender<()>,
    },
}

pub fn socket_path(prefix: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/fold-swarm-{}.sock", prefix))
}

pub async fn run_socket_server(
    path: PathBuf,
    cmd_tx: mpsc::Sender<SocketCommand>,
) -> Result<()> {
    // Remove existing socket
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    tracing::info!(path = %path.display(), "Socket server listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let cmd_tx = cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, cmd_tx).await {
                tracing::warn!(error = %e, "Socket connection error");
            }
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    cmd_tx: mpsc::Sender<SocketCommand>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let request: Request = serde_json::from_str(&line)?;
        let response = handle_request(request, &cmd_tx).await;
        let response_json = serde_json::to_string(&response)? + "\n";
        writer.write_all(response_json.as_bytes()).await?;
        line.clear();
    }

    Ok(())
}

async fn handle_request(request: Request, cmd_tx: &mpsc::Sender<SocketCommand>) -> Response {
    match request {
        Request::List => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            if cmd_tx.send(SocketCommand::List { reply }).await.is_err() {
                return Response {
                    success: false,
                    error: Some("Supervisor unavailable".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                };
            }
            match rx.await {
                Ok(workspaces) => Response {
                    success: true,
                    error: None,
                    workspaces: Some(workspaces),
                    agent_id: None,
                    status: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
            }
        }
        Request::Create { name } => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            if cmd_tx
                .send(SocketCommand::Create {
                    name: name.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                return Response {
                    success: false,
                    error: Some("Supervisor unavailable".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                };
            }
            match rx.await {
                Ok(Ok(agent_id)) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: Some(agent_id),
                    status: None,
                },
                Ok(Err(e)) => Response {
                    success: false,
                    error: Some(e.to_string()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
            }
        }
        Request::Delete { name } => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            if cmd_tx
                .send(SocketCommand::Delete { name, reply })
                .await
                .is_err()
            {
                return Response {
                    success: false,
                    error: Some("Supervisor unavailable".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                };
            }
            match rx.await {
                Ok(Ok(())) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
                Ok(Err(e)) => Response {
                    success: false,
                    error: Some(e.to_string()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
            }
        }
        Request::Status => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            if cmd_tx.send(SocketCommand::Status { reply }).await.is_err() {
                return Response {
                    success: false,
                    error: Some("Supervisor unavailable".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                };
            }
            match rx.await {
                Ok(status_info) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: None,
                    status: Some(status_info),
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
            }
        }
        Request::Stop => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            if cmd_tx.send(SocketCommand::Stop { reply }).await.is_err() {
                return Response {
                    success: false,
                    error: Some("Supervisor unavailable".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                };
            }
            match rx.await {
                Ok(()) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                    status: None,
                },
            }
        }
    }
}

/// Socket client for connecting to a running supervisor
pub struct SocketClient {
    stream: UnixStream,
}

impl SocketClient {
    /// Connect to supervisor socket
    pub async fn connect(prefix: &str) -> Result<Self> {
        let path = socket_path(prefix);
        let stream = UnixStream::connect(&path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to supervisor at {}: {}", path.display(), e))?;
        Ok(Self { stream })
    }

    /// Send a request and get a response
    async fn send_request(&mut self, request: Request) -> Result<Response> {
        let (reader, mut writer) = self.stream.split();

        let request_json = serde_json::to_string(&request)? + "\n";
        writer.write_all(request_json.as_bytes()).await?;

        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }

    /// Get status of running agents
    pub async fn status(&mut self) -> Result<StatusInfo> {
        let response = self.send_request(Request::Status).await?;
        if response.success {
            response.status.ok_or_else(|| anyhow::anyhow!("No status in response"))
        } else {
            Err(anyhow::anyhow!(
                response.error.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }

    /// Stop the supervisor
    pub async fn stop(&mut self) -> Result<()> {
        let response = self.send_request(Request::Stop).await?;
        if response.success {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                response.error.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }

    /// List running workspaces
    pub async fn list(&mut self) -> Result<Vec<String>> {
        let response = self.send_request(Request::List).await?;
        if response.success {
            response.workspaces.ok_or_else(|| anyhow::anyhow!("No workspaces in response"))
        } else {
            Err(anyhow::anyhow!(
                response.error.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }
}
