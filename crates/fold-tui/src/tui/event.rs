// ABOUTME: TUI event types and event stream.
// ABOUTME: Wraps crossterm events for cleaner handling.

#![allow(dead_code)]

use crossterm::event::{self, Event, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::Stream;

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize(u16, u16),
    Tick,
}

pub struct EventStream {
    rx: UnboundedReceiverStream<TuiEvent>,
}

impl EventStream {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn event polling task
        tokio::spawn(async move {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        let tui_event = match evt {
                            Event::Key(key) => Some(TuiEvent::Key(key)),
                            Event::Paste(text) => Some(TuiEvent::Paste(text)),
                            Event::Resize(w, h) => Some(TuiEvent::Resize(w, h)),
                            _ => None,
                        };

                        if let Some(e) = tui_event {
                            if tx.send(e).is_err() {
                                break;
                            }
                        }
                    }
                } else {
                    // Tick for animations
                    if tx.send(TuiEvent::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            rx: UnboundedReceiverStream::new(rx),
        }
    }
}

impl Stream for EventStream {
    type Item = TuiEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.rx).poll_next(cx)
    }
}
