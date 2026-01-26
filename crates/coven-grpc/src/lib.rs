// ABOUTME: Shared gRPC client utilities for coven-agent, coven-swarm, and coven-leader.
// ABOUTME: Provides channel creation, registration retry logic, bidirectional streaming, and message handling.

pub mod channel;
pub mod error;
pub mod handler;
pub mod registration;
pub mod stream;

// Channel creation
pub use channel::{create_channel, create_simple_channel, ChannelConfig, KeepAliveConfig};

// Error types
pub use error::GrpcClientError;

// Registration
pub use registration::{
    is_name_collision, is_name_collision_message, RegistrationConfig, RegistrationOutcome,
    RegistrationState, MAX_REGISTRATION_ATTEMPTS,
};

// Stream management
pub use stream::{
    BidirectionalStream, OutboundStream, StreamReceiver, StreamSender, DEFAULT_CHANNEL_BUFFER,
};

// Message handling
pub use handler::{CallbackHandler, HandleOutcome, HandlerContext, MessageHandler};

// Re-export proto types for convenience
pub use coven_proto;
