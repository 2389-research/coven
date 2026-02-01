// ABOUTME: gRPC channel creation with keep-alive and TLS configuration.
// ABOUTME: Provides configurable channel builder for coven gRPC connections.

use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use crate::error::GrpcClientError;

/// Configuration for gRPC channel keep-alive behavior.
#[derive(Debug, Clone)]
pub struct KeepAliveConfig {
    /// Interval between keep-alive pings when the connection is idle.
    pub interval: Duration,
    /// Timeout waiting for keep-alive response before considering connection dead.
    pub timeout: Duration,
    /// Whether to send keep-alive pings even when no streams are active.
    pub while_idle: bool,
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(20),
            while_idle: true,
        }
    }
}

/// Configuration for creating a gRPC channel.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Server address to connect to (e.g., "http://localhost:50051").
    pub address: String,
    /// Keep-alive configuration. If None, keep-alive is disabled.
    pub keep_alive: Option<KeepAliveConfig>,
    /// Connection timeout.
    pub connect_timeout: Option<Duration>,
    /// Enable TLS for the connection.
    pub use_tls: bool,
}

impl ChannelConfig {
    /// Create a channel config with default settings.
    /// Auto-detects TLS from URL scheme (https:// enables TLS).
    pub fn new(address: impl Into<String>) -> Self {
        let addr = address.into().trim().to_string();
        let use_tls = Self::detect_tls(&addr);
        Self {
            address: addr,
            keep_alive: Some(KeepAliveConfig::default()),
            connect_timeout: Some(Duration::from_secs(30)),
            use_tls,
        }
    }

    /// Detect TLS from URL scheme (case-insensitive).
    fn detect_tls(addr: &str) -> bool {
        addr.to_lowercase().starts_with("https://")
    }

    /// Normalize scheme to match TLS setting.
    fn normalize_scheme(addr: &str, use_tls: bool) -> String {
        let lower = addr.to_lowercase();
        if use_tls && lower.starts_with("http://") {
            format!("https://{}", &addr[7..])
        } else if !use_tls && lower.starts_with("https://") {
            format!("http://{}", &addr[8..])
        } else {
            addr.to_string()
        }
    }

    /// Disable keep-alive.
    pub fn without_keep_alive(mut self) -> Self {
        self.keep_alive = None;
        self
    }

    /// Set custom keep-alive configuration.
    pub fn with_keep_alive(mut self, config: KeepAliveConfig) -> Self {
        self.keep_alive = Some(config);
        self
    }

    /// Set connection timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Enable TLS for the connection.
    /// Also normalizes the address scheme to https:// if it was http://.
    pub fn with_tls(mut self) -> Self {
        self.use_tls = true;
        self.address = Self::normalize_scheme(&self.address, true);
        self
    }

    /// Disable TLS for the connection.
    /// Also normalizes the address scheme to http:// if it was https://.
    pub fn without_tls(mut self) -> Self {
        self.use_tls = false;
        self.address = Self::normalize_scheme(&self.address, false);
        self
    }
}

/// Create a gRPC channel with the specified configuration.
///
/// Applies keep-alive and TLS settings if configured. Keep-alive is important for
/// long-lived streaming connections to detect dead peers and prevent
/// connection resets from load balancers.
pub async fn create_channel(config: &ChannelConfig) -> Result<Channel, GrpcClientError> {
    let mut endpoint = Endpoint::from_shared(config.address.clone())
        .map_err(|e| GrpcClientError::InvalidAddress(e.to_string()))?;

    // Apply TLS if configured
    if config.use_tls {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new())
            .map_err(|e| GrpcClientError::ConnectionFailed(format!("TLS config error: {}", e)))?;
    }

    // Apply keep-alive settings if configured
    if let Some(ka) = &config.keep_alive {
        endpoint = endpoint
            .http2_keep_alive_interval(ka.interval)
            .keep_alive_timeout(ka.timeout)
            .keep_alive_while_idle(ka.while_idle);
    }

    // Apply connection timeout if configured
    if let Some(timeout) = config.connect_timeout {
        endpoint = endpoint.connect_timeout(timeout);
    }

    let channel = endpoint
        .connect()
        .await
        .map_err(|e| GrpcClientError::ConnectionFailed(e.to_string()))?;

    tracing::debug!(
        address = %config.address,
        keep_alive = config.keep_alive.is_some(),
        use_tls = config.use_tls,
        "gRPC channel connected"
    );

    Ok(channel)
}

/// Create a simple channel without keep-alive (useful for one-shot operations).
pub async fn create_simple_channel(address: &str) -> Result<Channel, GrpcClientError> {
    let config = ChannelConfig::new(address).without_keep_alive();
    create_channel(&config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Install crypto provider for TLS tests (idempotent)
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_default_keep_alive() {
        let ka = KeepAliveConfig::default();
        assert_eq!(ka.interval, Duration::from_secs(10));
        assert_eq!(ka.timeout, Duration::from_secs(20));
        assert!(ka.while_idle);
    }

    #[test]
    fn test_channel_config_builder() {
        let config = ChannelConfig::new("http://localhost:50051")
            .with_connect_timeout(Duration::from_secs(10))
            .with_keep_alive(KeepAliveConfig {
                interval: Duration::from_secs(5),
                timeout: Duration::from_secs(10),
                while_idle: false,
            });

        assert_eq!(config.address, "http://localhost:50051");
        assert_eq!(config.connect_timeout, Some(Duration::from_secs(10)));
        let ka = config.keep_alive.unwrap();
        assert_eq!(ka.interval, Duration::from_secs(5));
        assert!(!ka.while_idle);
    }

    #[test]
    fn test_channel_config_without_keep_alive() {
        let config = ChannelConfig::new("http://localhost:50051").without_keep_alive();
        assert!(config.keep_alive.is_none());
    }

    #[test]
    fn test_channel_config_default_values() {
        let config = ChannelConfig::new("http://localhost:50051");
        assert_eq!(config.address, "http://localhost:50051");
        assert!(config.keep_alive.is_some());
        assert_eq!(config.connect_timeout, Some(Duration::from_secs(30)));

        // Check default keep-alive values
        let ka = config.keep_alive.unwrap();
        assert_eq!(ka.interval, Duration::from_secs(10));
        assert_eq!(ka.timeout, Duration::from_secs(20));
        assert!(ka.while_idle);
    }

    #[test]
    fn test_keep_alive_config_clone() {
        let ka = KeepAliveConfig::default();
        let cloned = ka.clone();
        assert_eq!(ka.interval, cloned.interval);
        assert_eq!(ka.timeout, cloned.timeout);
        assert_eq!(ka.while_idle, cloned.while_idle);
    }

    #[test]
    fn test_keep_alive_config_debug() {
        let ka = KeepAliveConfig::default();
        let debug_str = format!("{:?}", ka);
        assert!(debug_str.contains("KeepAliveConfig"));
        assert!(debug_str.contains("interval"));
        assert!(debug_str.contains("timeout"));
    }

    #[test]
    fn test_channel_config_clone() {
        let config = ChannelConfig::new("http://localhost:50051")
            .with_connect_timeout(Duration::from_secs(5));
        let cloned = config.clone();
        assert_eq!(config.address, cloned.address);
        assert_eq!(config.connect_timeout, cloned.connect_timeout);
    }

    #[test]
    fn test_channel_config_debug() {
        let config = ChannelConfig::new("http://localhost:50051");
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("ChannelConfig"));
        assert!(debug_str.contains("localhost"));
    }

    #[tokio::test]
    async fn test_create_channel_invalid_address() {
        // Empty string is clearly invalid
        let config = ChannelConfig::new("");
        let result = create_channel(&config).await;
        assert!(result.is_err());
        // May be InvalidAddress or ConnectionFailed depending on how tonic handles it
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                GrpcClientError::InvalidAddress(_) | GrpcClientError::ConnectionFailed(_)
            ),
            "expected InvalidAddress or ConnectionFailed, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_channel_connection_refused() {
        // Use a valid URL but unreachable port
        let config = ChannelConfig::new("http://127.0.0.1:1")
            .with_connect_timeout(Duration::from_millis(100));
        let result = create_channel(&config).await;
        // This should fail with connection refused
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GrpcClientError::ConnectionFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_create_simple_channel_connection_failure() {
        // Use an unreachable address
        let result = create_simple_channel("http://127.0.0.1:1").await;
        assert!(result.is_err());
        // Should fail to connect
        let err = result.unwrap_err();
        assert!(
            matches!(err, GrpcClientError::ConnectionFailed(_)),
            "expected ConnectionFailed, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_channel_with_keep_alive() {
        // Test that keep-alive config is applied (it will fail to connect but should not error on config)
        let ka_config = KeepAliveConfig {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            while_idle: false,
        };
        let config = ChannelConfig::new("http://127.0.0.1:1")
            .with_keep_alive(ka_config)
            .with_connect_timeout(Duration::from_millis(100));

        // Verify config was set correctly
        assert!(config.keep_alive.is_some());
        let ka = config.keep_alive.as_ref().unwrap();
        assert_eq!(ka.interval, Duration::from_secs(5));
        assert!(!ka.while_idle);

        // The actual connection will fail, but we've tested the config path
        let result = create_channel(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_channel_without_keep_alive() {
        // Test that no keep-alive config is applied
        let config = ChannelConfig::new("http://127.0.0.1:1")
            .without_keep_alive()
            .with_connect_timeout(Duration::from_millis(100));

        assert!(config.keep_alive.is_none());

        // The actual connection will fail, but we've tested the config path
        let result = create_channel(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_channel_without_connect_timeout() {
        // Test with no explicit connect_timeout (uses default)
        let mut config = ChannelConfig::new("not-a-valid-url");
        config.connect_timeout = None;

        let result = create_channel(&config).await;
        // Should fail on invalid address before timeout is relevant
        assert!(result.is_err());
    }

    #[test]
    fn test_tls_auto_detection_http() {
        let config = ChannelConfig::new("http://localhost:50051");
        assert!(!config.use_tls, "http:// should not enable TLS");
    }

    #[test]
    fn test_tls_auto_detection_https() {
        let config = ChannelConfig::new("https://localhost:50051");
        assert!(config.use_tls, "https:// should enable TLS");
    }

    #[test]
    fn test_with_tls_builder() {
        let config = ChannelConfig::new("http://localhost:50051").with_tls();
        assert!(config.use_tls, "with_tls() should enable TLS");
    }

    #[test]
    fn test_without_tls_builder() {
        let config = ChannelConfig::new("https://localhost:50051").without_tls();
        assert!(!config.use_tls, "without_tls() should disable TLS");
    }

    #[test]
    fn test_with_tls_normalizes_scheme() {
        let config = ChannelConfig::new("http://localhost:50051").with_tls();
        assert!(config.use_tls);
        assert!(
            config.address.starts_with("https://"),
            "with_tls() should normalize http:// to https://, got: {}",
            config.address
        );
    }

    #[test]
    fn test_without_tls_normalizes_scheme() {
        let config = ChannelConfig::new("https://localhost:50051").without_tls();
        assert!(!config.use_tls);
        assert!(
            config.address.starts_with("http://"),
            "without_tls() should normalize https:// to http://, got: {}",
            config.address
        );
    }

    #[test]
    fn test_tls_detection_case_insensitive() {
        let config1 = ChannelConfig::new("HTTPS://localhost:50051");
        assert!(config1.use_tls, "HTTPS:// should enable TLS");

        let config2 = ChannelConfig::new("HttpS://localhost:50051");
        assert!(config2.use_tls, "HttpS:// should enable TLS");

        let config3 = ChannelConfig::new("HTTP://localhost:50051");
        assert!(!config3.use_tls, "HTTP:// should not enable TLS");
    }

    #[test]
    fn test_tls_detection_trims_whitespace() {
        let config = ChannelConfig::new("  https://localhost:50051  ");
        assert!(
            config.use_tls,
            "should detect TLS after trimming whitespace"
        );
        assert_eq!(config.address, "https://localhost:50051");
    }

    #[test]
    fn test_scheme_normalization_preserves_path() {
        let config = ChannelConfig::new("http://example.com:8080/api/v1").with_tls();
        assert_eq!(config.address, "https://example.com:8080/api/v1");

        let config2 = ChannelConfig::new("https://example.com:443/path").without_tls();
        assert_eq!(config2.address, "http://example.com:443/path");
    }

    /// A plaintext TCP server that accepts connections and sends garbage.
    /// Used to test TLS handshake failures (TLS client connects to plaintext server).
    struct PlaintextServer {
        port: u16,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl PlaintextServer {
        fn start() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();

            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let shutdown_clone = shutdown.clone();

            let handle = std::thread::spawn(move || {
                while !shutdown_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok((mut stream, _)) = listener.accept() {
                        // Send plaintext garbage to fail TLS handshake
                        let _ = std::io::Write::write_all(&mut stream, b"NOT TLS\r\n");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            });

            PlaintextServer {
                port,
                shutdown,
                handle: Some(handle),
            }
        }
    }

    impl Drop for PlaintextServer {
        fn drop(&mut self) {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[tokio::test]
    async fn test_create_channel_with_tls_normalized_scheme() {
        ensure_crypto_provider();
        // Start plaintext server - TLS client will fail handshake
        let server = PlaintextServer::start();
        let addr = format!("http://127.0.0.1:{}", server.port);

        // Test that with_tls() normalizes scheme and flows through TLS config
        // without hitting InvalidAddress (scheme mismatch) error
        let config = ChannelConfig::new(&addr)
            .with_tls()
            .with_connect_timeout(Duration::from_millis(100));

        // Verify scheme was normalized
        assert!(config.address.starts_with("https://"));
        assert!(config.use_tls);

        // Should fail with ConnectionFailed (TLS handshake/connect failure),
        // not InvalidAddress (which would indicate scheme mismatch)
        let result = create_channel(&config).await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GrpcClientError::ConnectionFailed(_)),
            "TLS channel should fail with ConnectionFailed, not InvalidAddress"
        );

        // Explicitly drop server after await to prevent NLL from dropping early
        drop(server);
    }

    #[tokio::test]
    async fn test_create_channel_https_url_direct() {
        ensure_crypto_provider();
        // Start plaintext server - TLS client will fail handshake
        let server = PlaintextServer::start();
        let addr = format!("https://127.0.0.1:{}", server.port);

        // Test https:// URL flows through TLS path correctly
        let config = ChannelConfig::new(&addr).with_connect_timeout(Duration::from_millis(100));

        assert!(config.use_tls);

        let result = create_channel(&config).await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GrpcClientError::ConnectionFailed(_)),
            "HTTPS channel should fail with ConnectionFailed"
        );

        // Explicitly drop server after await to prevent NLL from dropping early
        drop(server);
    }
}
