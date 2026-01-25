// ABOUTME: Claude SDK backend implementation using claude-sdk-rs
// ABOUTME: Wraps the Claude Code CLI for message processing with streaming

use super::{Backend, BackendEvent};
use crate::config::ClaudeConfig;
use anyhow::Result;
use async_trait::async_trait;
use claude_sdk_rs::{Client, Config, SessionId};
use futures::StreamExt;
use futures::stream::BoxStream;

pub struct ClaudeSdkBackend {
    client: Client,
}

impl ClaudeSdkBackend {
    pub fn new(config: &ClaudeConfig) -> Result<Self> {
        let mut builder = Config::builder().timeout_secs(config.timeout_secs);

        if let Some(ref prompt) = config.system_prompt {
            builder = builder.system_prompt(prompt);
        }

        let client = Client::new(builder.build()?);
        Ok(Self { client })
    }
}

#[async_trait]
impl Backend for ClaudeSdkBackend {
    fn name(&self) -> &'static str {
        "claude-sdk"
    }

    async fn send(
        &self,
        session_id: &str,
        message: &str,
        _is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>> {
        let client = self.client.clone();
        let session = SessionId::new(session_id.to_string());
        let content = message.to_string();

        let (tx, rx) = tokio::sync::mpsc::channel::<BackendEvent>(100);

        tokio::spawn(async move {
            // Send thinking indicator
            let _ = tx.send(BackendEvent::Thinking).await;

            // Start streaming
            match client.query(&content).session(session).stream().await {
                Ok(mut stream) => {
                    let mut full_response = String::new();

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(message) => {
                                // Check if this is an assistant message (not stats/result)
                                let text = message.content();
                                // Filter out "Conversation stats:" messages
                                if !text.is_empty() && !text.starts_with("Conversation stats:") {
                                    full_response.push_str(&text);
                                    let _ = tx.send(BackendEvent::Text(text)).await;
                                }
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(BackendEvent::Error(format!("Stream error: {}", e)))
                                    .await;
                                let _ = tx
                                    .send(BackendEvent::Done {
                                        full_response: full_response.clone(),
                                    })
                                    .await;
                                return;
                            }
                        }
                    }

                    let _ = tx.send(BackendEvent::Done { full_response }).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(BackendEvent::Error(format!("Claude error: {}", e)))
                        .await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}
