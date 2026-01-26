// ABOUTME: Frame requester for on-demand redraws.
// ABOUTME: Allows widgets to request UI updates.

#![allow(dead_code)]

use tokio::sync::broadcast;

#[derive(Clone)]
pub struct FrameRequester {
    tx: broadcast::Sender<()>,
}

impl FrameRequester {
    pub fn new() -> (Self, broadcast::Receiver<()>) {
        let (tx, rx) = broadcast::channel(16);
        (Self { tx }, rx)
    }

    pub fn request_frame(&self) {
        let _ = self.tx.send(());
    }
}
