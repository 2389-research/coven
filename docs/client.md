# Client Libraries

Libraries for connecting to coven-gateway from applications.

## Overview

| Crate | Description |
|-------|-------------|
| `coven-client` | High-level gateway client (Rust, Swift, Kotlin) |
| `coven-grpc` | Low-level gRPC utilities |
| `coven-ssh` | SSH key authentication |

## coven-client

High-level client for gateway communication, with UniFFI bindings for mobile.

### Rust Usage

```rust
use coven_client::{Client, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create client
    let config = Config {
        gateway_url: "localhost:50051".to_string(),
        ..Default::default()
    };
    let client = Client::new(config).await?;

    // Send message
    let response = client.send_message("thread-123", "Hello!").await?;

    // Stream events
    let mut stream = client.stream_events("thread-123").await?;
    while let Some(event) = stream.next().await {
        match event {
            Event::Text(text) => println!("{}", text),
            Event::Done => break,
            _ => {}
        }
    }

    Ok(())
}
```

### Swift Usage (iOS/macOS)

```swift
import CovenClient

let client = try await Client(gatewayUrl: "localhost:50051")

// Send message
let response = try await client.sendMessage(
    threadId: "thread-123",
    message: "Hello!"
)

// Stream events
for try await event in client.streamEvents(threadId: "thread-123") {
    switch event {
    case .text(let text):
        print(text)
    case .done:
        break
    default:
        continue
    }
}
```

### Building UniFFI Bindings

```bash
# Generate Swift bindings
make bindings

# Output in crates/coven-client/bindings/
# - coven_client.swift
# - coven_clientFFI.h
# - coven_clientFFI.modulemap
```

### XCFramework (iOS)

```bash
# Build XCFramework for iOS
./scripts/build-xcframework.sh

# Output: CovenClient.xcframework
```

## coven-grpc

Low-level gRPC client utilities shared across crates.

### Channel Creation

```rust
use coven_grpc::channel::create_channel;

let channel = create_channel("localhost:50051").await?;
```

### Registration Retry

```rust
use coven_grpc::registration::register_with_retry;

let (session_id, stream) = register_with_retry(
    channel,
    agent_metadata,
    max_retries,
).await?;
```

### Bidirectional Streaming

```rust
use coven_grpc::stream::BidiStream;

let stream = BidiStream::new(grpc_stream);

// Send messages
stream.send(agent_message).await?;

// Receive messages
while let Some(server_message) = stream.recv().await? {
    // Handle message
}
```

### Error Handling

```rust
use coven_grpc::error::GrpcError;

match result {
    Err(GrpcError::ConnectionFailed(e)) => {
        // Retry connection
    }
    Err(GrpcError::AuthenticationFailed) => {
        // Check credentials
    }
    Err(GrpcError::Timeout) => {
        // Retry or fail
    }
    _ => {}
}
```

## coven-ssh

SSH key management for authentication.

### Key Loading

```rust
use coven_ssh::{load_key, KeyType};

// Load default key
let key = load_key(None)?;  // Uses ~/.ssh/id_ed25519

// Load specific key
let key = load_key(Some("/path/to/key"))?;

// Get key type
match key.key_type() {
    KeyType::Ed25519 => println!("ED25519 key"),
    KeyType::Rsa => println!("RSA key"),
}
```

### Fingerprinting

```rust
use coven_ssh::fingerprint;

let fp = fingerprint(&key)?;
println!("SHA256:{}", fp);
```

### gRPC Credentials

```rust
use coven_ssh::credentials::SshCredentials;

let creds = SshCredentials::new(key)?;
let channel = create_channel_with_auth("localhost:50051", creds).await?;
```

### Supported Key Types

| Type | File |
|------|------|
| ED25519 | `~/.ssh/id_ed25519` |
| RSA | `~/.ssh/id_rsa` |
| ECDSA | `~/.ssh/id_ecdsa` |

## Configuration

### Client Config

```rust
pub struct Config {
    /// Gateway gRPC address
    pub gateway_url: String,

    /// SSH key path (None = default)
    pub ssh_key_path: Option<String>,

    /// Connection timeout
    pub timeout: Duration,

    /// Enable TLS
    pub tls: bool,
}
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `COVEN_GATEWAY` | Gateway address |
| `COVEN_SSH_KEY` | SSH key path |
| `COVEN_TLS` | Enable TLS (true/false) |

## Error Types

### coven-client

```rust
pub enum ClientError {
    ConnectionFailed(String),
    AuthenticationFailed,
    SendFailed(String),
    StreamError(String),
    Timeout,
}
```

### coven-grpc

```rust
pub enum GrpcError {
    ConnectionFailed(tonic::transport::Error),
    AuthenticationFailed,
    StreamClosed,
    Timeout,
    InvalidMessage(String),
}
```

### coven-ssh

```rust
pub enum SshError {
    KeyNotFound(PathBuf),
    InvalidKeyFormat,
    UnsupportedKeyType,
    PermissionDenied,
}
```

## Thread Safety

All client types are `Send + Sync` and safe for concurrent use:

```rust
let client = Arc::new(Client::new(config).await?);

// Use from multiple tasks
let client1 = client.clone();
tokio::spawn(async move {
    client1.send_message("thread-1", "Hello").await
});

let client2 = client.clone();
tokio::spawn(async move {
    client2.send_message("thread-2", "World").await
});
```

## See Also

- [Architecture](architecture.md) - System overview
- [coven-tui](tui.md) - TUI using coven-client
- [Protocol](architecture.md#protocol) - gRPC protocol details
