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
                };
            }
            match rx.await {
                Ok(workspaces) => Response {
                    success: true,
                    error: None,
                    workspaces: Some(workspaces),
                    agent_id: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
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
                };
            }
            match rx.await {
                Ok(Ok(agent_id)) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: Some(agent_id),
                },
                Ok(Err(e)) => Response {
                    success: false,
                    error: Some(e.to_string()),
                    workspaces: None,
                    agent_id: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
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
                };
            }
            match rx.await {
                Ok(Ok(())) => Response {
                    success: true,
                    error: None,
                    workspaces: None,
                    agent_id: None,
                },
                Ok(Err(e)) => Response {
                    success: false,
                    error: Some(e.to_string()),
                    workspaces: None,
                    agent_id: None,
                },
                Err(_) => Response {
                    success: false,
                    error: Some("No response".into()),
                    workspaces: None,
                    agent_id: None,
                },
            }
        }
    }
}
