// ABOUTME: Generic message handler trait for processing server messages.
// ABOUTME: Allows each project to implement custom message handling logic.

use std::future::Future;
use std::pin::Pin;

use crate::error::GrpcClientError;

/// Context provided to message handlers.
///
/// Contains information about the current connection and request.
#[derive(Debug, Clone)]
pub struct HandlerContext {
    /// The registered agent ID (may include suffix).
    pub agent_id: String,
    /// The instance ID assigned by the server.
    pub instance_id: String,
    /// The server ID.
    pub server_id: String,
}

impl HandlerContext {
    /// Create a handler context.
    pub fn new(agent_id: String, instance_id: String, server_id: String) -> Self {
        Self {
            agent_id,
            instance_id,
            server_id,
        }
    }
}

/// Outcome of handling a message.
#[derive(Debug)]
pub enum HandleOutcome {
    /// Message handled successfully, continue processing.
    Continue,
    /// Handler requests shutdown.
    Shutdown { reason: String },
    /// Handler encountered an error but wants to continue.
    Error { error: String, continue_: bool },
}

impl HandleOutcome {
    /// Create a continue outcome.
    pub fn ok() -> Self {
        Self::Continue
    }

    /// Create a shutdown outcome.
    pub fn shutdown(reason: impl Into<String>) -> Self {
        Self::Shutdown {
            reason: reason.into(),
        }
    }

    /// Create a recoverable error outcome.
    pub fn recoverable_error(error: impl Into<String>) -> Self {
        Self::Error {
            error: error.into(),
            continue_: true,
        }
    }

    /// Create a fatal error outcome.
    pub fn fatal_error(error: impl Into<String>) -> Self {
        Self::Error {
            error: error.into(),
            continue_: false,
        }
    }
}

/// Trait for handling incoming server messages.
///
/// Implement this trait to define how your agent processes different
/// message types from the coven-gateway server.
///
/// # Type Parameters
///
/// * `TServerMsg` - The server message type (usually `ServerMessage` from proto)
/// * `TAgentMsg` - The agent message type for responses (usually `AgentMessage` from proto)
///
/// # Example
///
/// ```ignore
/// struct MyHandler {
///     backend: Arc<dyn Backend>,
/// }
///
/// #[async_trait]
/// impl MessageHandler<ServerMessage, AgentMessage> for MyHandler {
///     async fn on_message(
///         &mut self,
///         ctx: &HandlerContext,
///         msg: ServerMessage,
///         sender: &StreamSender<AgentMessage>,
///     ) -> Result<HandleOutcome, GrpcClientError> {
///         match msg.payload {
///             Some(Payload::SendMessage(send_msg)) => {
///                 // Process the message with your backend
///                 let response = self.backend.handle(send_msg).await?;
///                 sender.send(response).await?;
///                 Ok(HandleOutcome::ok())
///             }
///             Some(Payload::Shutdown(shutdown)) => {
///                 Ok(HandleOutcome::shutdown(shutdown.reason))
///             }
///             _ => Ok(HandleOutcome::ok())
///         }
///     }
/// }
/// ```
pub trait MessageHandler<TServerMsg, TAgentMsg>: Send {
    /// Handle an incoming server message.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Context about the current connection
    /// * `msg` - The incoming server message
    /// * `sender` - Sender for responding to the server
    ///
    /// # Returns
    ///
    /// * `Ok(HandleOutcome::Continue)` - Message handled, continue processing
    /// * `Ok(HandleOutcome::Shutdown { reason })` - Request clean shutdown
    /// * `Ok(HandleOutcome::Error { error, continue_ })` - Error occurred
    /// * `Err(_)` - Fatal error, abort connection
    fn on_message<'a>(
        &'a mut self,
        ctx: &'a HandlerContext,
        msg: TServerMsg,
        sender: &'a crate::stream::StreamSender<TAgentMsg>,
    ) -> Pin<Box<dyn Future<Output = Result<HandleOutcome, GrpcClientError>> + Send + 'a>>;

    /// Called when registration completes successfully.
    ///
    /// Override to perform setup after registration.
    fn on_registered<'a>(
        &'a mut self,
        _ctx: &'a HandlerContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), GrpcClientError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Called when the stream is closing.
    ///
    /// Override to perform cleanup.
    fn on_closing<'a>(
        &'a mut self,
        _ctx: &'a HandlerContext,
        _reason: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}

/// A simple callback-based message handler.
///
/// Useful for simple use cases where you just want to provide a closure
/// to handle messages rather than implementing the full trait.
pub struct CallbackHandler<TServerMsg, TAgentMsg, F, Fut>
where
    F: FnMut(&HandlerContext, TServerMsg, &crate::stream::StreamSender<TAgentMsg>) -> Fut + Send,
    Fut: Future<Output = Result<HandleOutcome, GrpcClientError>> + Send,
{
    callback: F,
    _phantom: std::marker::PhantomData<(TServerMsg, TAgentMsg)>,
}

impl<TServerMsg, TAgentMsg, F, Fut> CallbackHandler<TServerMsg, TAgentMsg, F, Fut>
where
    F: FnMut(&HandlerContext, TServerMsg, &crate::stream::StreamSender<TAgentMsg>) -> Fut + Send,
    Fut: Future<Output = Result<HandleOutcome, GrpcClientError>> + Send,
{
    /// Create a callback handler from a closure.
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<TServerMsg, TAgentMsg, F, Fut> MessageHandler<TServerMsg, TAgentMsg>
    for CallbackHandler<TServerMsg, TAgentMsg, F, Fut>
where
    TServerMsg: Send + 'static,
    TAgentMsg: Send + 'static,
    F: FnMut(&HandlerContext, TServerMsg, &crate::stream::StreamSender<TAgentMsg>) -> Fut + Send,
    Fut: Future<Output = Result<HandleOutcome, GrpcClientError>> + Send,
{
    fn on_message<'a>(
        &'a mut self,
        ctx: &'a HandlerContext,
        msg: TServerMsg,
        sender: &'a crate::stream::StreamSender<TAgentMsg>,
    ) -> Pin<Box<dyn Future<Output = Result<HandleOutcome, GrpcClientError>> + Send + 'a>> {
        Box::pin((self.callback)(ctx, msg, sender))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_context() {
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "abc123".to_string(),
            "server-1".to_string(),
        );
        assert_eq!(ctx.agent_id, "test-agent");
        assert_eq!(ctx.instance_id, "abc123");
        assert_eq!(ctx.server_id, "server-1");
    }

    #[test]
    fn test_handle_outcome_variants() {
        let ok = HandleOutcome::ok();
        assert!(matches!(ok, HandleOutcome::Continue));

        let shutdown = HandleOutcome::shutdown("done");
        assert!(matches!(shutdown, HandleOutcome::Shutdown { reason } if reason == "done"));

        let recoverable = HandleOutcome::recoverable_error("oops");
        assert!(
            matches!(recoverable, HandleOutcome::Error { error, continue_: true } if error == "oops")
        );

        let fatal = HandleOutcome::fatal_error("crash");
        assert!(
            matches!(fatal, HandleOutcome::Error { error, continue_: false } if error == "crash")
        );
    }

    #[test]
    fn test_handler_context_clone() {
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "abc123".to_string(),
            "server-1".to_string(),
        );
        let cloned = ctx.clone();

        assert_eq!(ctx.agent_id, cloned.agent_id);
        assert_eq!(ctx.instance_id, cloned.instance_id);
        assert_eq!(ctx.server_id, cloned.server_id);
    }

    #[test]
    fn test_handler_context_debug() {
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "abc123".to_string(),
            "server-1".to_string(),
        );
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("test-agent"));
        assert!(debug_str.contains("abc123"));
        assert!(debug_str.contains("server-1"));
    }

    #[test]
    fn test_handle_outcome_debug() {
        let ok = HandleOutcome::ok();
        let debug_str = format!("{:?}", ok);
        assert!(debug_str.contains("Continue"));

        let shutdown = HandleOutcome::shutdown("done");
        let debug_str = format!("{:?}", shutdown);
        assert!(debug_str.contains("Shutdown"));
        assert!(debug_str.contains("done"));

        let error = HandleOutcome::recoverable_error("oops");
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("Error"));
        assert!(debug_str.contains("oops"));
    }

    #[tokio::test]
    async fn test_callback_handler_basic() {
        use crate::stream::StreamSender;
        use tokio::sync::mpsc;

        let (tx, _rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "instance-1".to_string(),
            "server-1".to_string(),
        );

        // Create a callback handler that just returns Continue
        let mut handler = CallbackHandler::new(
            |_ctx: &HandlerContext, msg: String, _sender: &StreamSender<String>| async move {
                assert_eq!(msg, "test message");
                Ok(HandleOutcome::ok())
            },
        );

        // Call on_message
        let result = handler
            .on_message(&ctx, "test message".to_string(), &sender)
            .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HandleOutcome::Continue));
    }

    #[tokio::test]
    async fn test_callback_handler_shutdown() {
        use crate::stream::StreamSender;
        use tokio::sync::mpsc;

        let (tx, _rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "instance-1".to_string(),
            "server-1".to_string(),
        );

        // Create a callback handler that returns Shutdown
        let mut handler = CallbackHandler::new(
            |_ctx: &HandlerContext, _msg: String, _sender: &StreamSender<String>| async move {
                Ok(HandleOutcome::shutdown("user requested"))
            },
        );

        let result = handler.on_message(&ctx, "any".to_string(), &sender).await;
        assert!(result.is_ok());
        assert!(
            matches!(result.unwrap(), HandleOutcome::Shutdown { reason } if reason == "user requested")
        );
    }

    /// A test handler that implements the trait to test default methods.
    struct TestHandler;

    impl MessageHandler<String, String> for TestHandler {
        fn on_message<'a>(
            &'a mut self,
            _ctx: &'a HandlerContext,
            _msg: String,
            _sender: &'a crate::stream::StreamSender<String>,
        ) -> Pin<Box<dyn Future<Output = Result<HandleOutcome, GrpcClientError>> + Send + 'a>>
        {
            Box::pin(async { Ok(HandleOutcome::ok()) })
        }
    }

    #[tokio::test]
    async fn test_default_on_registered() {
        let mut handler = TestHandler;
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "instance-1".to_string(),
            "server-1".to_string(),
        );

        let result = handler.on_registered(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_default_on_closing() {
        let mut handler = TestHandler;
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "instance-1".to_string(),
            "server-1".to_string(),
        );

        // Should complete without error
        handler.on_closing(&ctx, Some("test reason")).await;
        handler.on_closing(&ctx, None).await;
    }

    #[tokio::test]
    async fn test_test_handler_on_message() {
        use crate::stream::StreamSender;
        use tokio::sync::mpsc;

        let mut handler = TestHandler;
        let ctx = HandlerContext::new(
            "test-agent".to_string(),
            "instance-1".to_string(),
            "server-1".to_string(),
        );

        let (tx, _rx) = mpsc::channel::<String>(10);
        let sender = StreamSender::new(tx);

        // Call on_message on the TestHandler
        let result = handler
            .on_message(&ctx, "test message".to_string(), &sender)
            .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HandleOutcome::Continue));
    }
}
