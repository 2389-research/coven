// ABOUTME: Bidirectional gRPC stream management for agent communication.
// ABOUTME: Provides typed sender/receiver wrappers and stream creation utilities.

use std::pin::Pin;

use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Streaming;

use crate::error::GrpcClientError;

/// Default buffer size for outbound message channels.
pub const DEFAULT_CHANNEL_BUFFER: usize = 100;

/// Sender half of a bidirectional stream.
///
/// Wraps an mpsc sender for outgoing messages with convenience methods.
#[derive(Debug, Clone)]
pub struct StreamSender<T> {
    inner: mpsc::Sender<T>,
}

impl<T> StreamSender<T> {
    /// Create a stream sender from an mpsc sender.
    pub fn new(sender: mpsc::Sender<T>) -> Self {
        Self { inner: sender }
    }

    /// Send a message on the stream.
    pub async fn send(&self, msg: T) -> Result<(), GrpcClientError> {
        self.inner
            .send(msg)
            .await
            .map_err(|_| GrpcClientError::StreamClosed)
    }

    /// Try to send a message without waiting.
    pub fn try_send(&self, msg: T) -> Result<(), GrpcClientError> {
        self.inner
            .try_send(msg)
            .map_err(|_| GrpcClientError::StreamClosed)
    }

    /// Check if the stream is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Get the capacity of the underlying channel.
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Get the raw mpsc sender (for advanced use cases).
    pub fn into_inner(self) -> mpsc::Sender<T> {
        self.inner
    }
}

/// Receiver half of a bidirectional stream.
///
/// Wraps a tonic Streaming with convenience methods.
pub struct StreamReceiver<T> {
    inner: Streaming<T>,
}

impl<T> StreamReceiver<T> {
    /// Create a stream receiver from a tonic Streaming.
    pub fn new(streaming: Streaming<T>) -> Self {
        Self { inner: streaming }
    }

    /// Receive the next message from the stream.
    pub async fn recv(&mut self) -> Result<Option<T>, GrpcClientError> {
        self.inner
            .message()
            .await
            .map_err(|e| GrpcClientError::StreamError(e.to_string()))
    }

    /// Get the raw tonic Streaming (for advanced use cases).
    pub fn into_inner(self) -> Streaming<T> {
        self.inner
    }
}

impl<T> Stream for StreamReceiver<T> {
    type Item = Result<T, GrpcClientError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner)
            .poll_next(cx)
            .map(|opt| opt.map(|res| res.map_err(|e| GrpcClientError::StreamError(e.to_string()))))
    }
}

/// A pair of sender and outbound stream for initiating bidirectional communication.
///
/// The outbound stream should be passed to the gRPC client method,
/// while the sender is used to send messages.
pub struct OutboundStream<T> {
    /// Sender for pushing messages to the stream.
    pub sender: StreamSender<T>,
    /// The stream to pass to the gRPC method.
    pub stream: ReceiverStream<T>,
}

impl<T> OutboundStream<T> {
    /// Create a outbound stream pair with the specified buffer size.
    pub fn new(buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        Self {
            sender: StreamSender::new(tx),
            stream: ReceiverStream::new(rx),
        }
    }

    /// Create a outbound stream pair with the default buffer size.
    pub fn with_default_buffer() -> Self {
        Self::new(DEFAULT_CHANNEL_BUFFER)
    }
}

/// A complete bidirectional stream pair after connection is established.
pub struct BidirectionalStream<TSend, TRecv> {
    /// Sender for outgoing messages.
    pub sender: StreamSender<TSend>,
    /// Receiver for incoming messages.
    pub receiver: StreamReceiver<TRecv>,
}

impl<TSend, TRecv> BidirectionalStream<TSend, TRecv> {
    /// Create a bidirectional stream from sender and receiver.
    pub fn new(sender: StreamSender<TSend>, receiver: StreamReceiver<TRecv>) -> Self {
        Self { sender, receiver }
    }

    /// Split into sender and receiver.
    pub fn split(self) -> (StreamSender<TSend>, StreamReceiver<TRecv>) {
        (self.sender, self.receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_outbound_stream_creation() {
        let outbound: OutboundStream<String> = OutboundStream::new(32);
        assert!(!outbound.sender.is_closed());
        assert_eq!(outbound.sender.capacity(), 32);
    }

    #[tokio::test]
    async fn test_stream_sender_send() {
        let (tx, mut rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        sender.send("hello".to_string()).await.unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received, "hello");
    }

    #[tokio::test]
    async fn test_stream_sender_closed_detection() {
        let (tx, rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        assert!(!sender.is_closed());
        drop(rx);
        assert!(sender.is_closed());
    }

    #[test]
    fn test_default_channel_buffer() {
        let outbound: OutboundStream<String> = OutboundStream::with_default_buffer();
        assert_eq!(outbound.sender.capacity(), DEFAULT_CHANNEL_BUFFER);
    }

    #[test]
    fn test_stream_sender_try_send() {
        let (tx, mut rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        // try_send should succeed when channel has capacity
        sender.try_send("hello".to_string()).unwrap();

        // Verify the message was sent
        let received = rx.try_recv().unwrap();
        assert_eq!(received, "hello");
    }

    #[test]
    fn test_stream_sender_try_send_closed() {
        let (tx, rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        // Drop receiver to close the channel
        drop(rx);

        // try_send should fail when channel is closed
        let result = sender.try_send("hello".to_string());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GrpcClientError::StreamClosed));
    }

    #[tokio::test]
    async fn test_stream_sender_send_closed() {
        let (tx, rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        // Drop receiver to close the channel
        drop(rx);

        // send should fail when channel is closed
        let result = sender.send("hello".to_string()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GrpcClientError::StreamClosed));
    }

    #[test]
    fn test_stream_sender_into_inner() {
        let (tx, _rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        // Get the inner sender
        let inner = sender.into_inner();

        // Verify it's the same sender by checking it's not closed
        assert!(!inner.is_closed());
    }

    #[test]
    fn test_outbound_stream_sender_clone() {
        let outbound: OutboundStream<String> = OutboundStream::new(10);
        let sender1 = outbound.sender.clone();
        let sender2 = sender1.clone();

        // Both senders should work
        assert!(!sender1.is_closed());
        assert!(!sender2.is_closed());
    }

    #[test]
    fn test_bidirectional_stream_new_and_split() {
        // We can't easily create a Streaming<T> without a real gRPC connection,
        // but we can test the OutboundStream components that don't require it.
        let outbound: OutboundStream<String> = OutboundStream::new(10);
        assert!(!outbound.sender.is_closed());

        // Test that the stream field exists and is usable
        let _stream = outbound.stream;
    }

    #[test]
    fn test_default_channel_buffer_constant() {
        // Verify the constant value
        assert_eq!(DEFAULT_CHANNEL_BUFFER, 100);
    }
}
