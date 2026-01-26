// ABOUTME: gRPC client utilities with JWT auth injection
// ABOUTME: Provides interceptor for adding Authorization header to requests

use tonic::service::Interceptor;

/// Interceptor that adds JWT Bearer token to requests
#[derive(Clone)]
pub struct AuthInterceptor {
    token: Option<String>,
}

impl AuthInterceptor {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref token) = self.token {
            let value = format!("Bearer {}", token)
                .parse()
                .map_err(|_| tonic::Status::internal("invalid token format"))?;
            req.metadata_mut().insert("authorization", value);
        }
        Ok(req)
    }
}
