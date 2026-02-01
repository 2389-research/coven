// ABOUTME: Implementation of 'coven-admin token' commands
// ABOUTME: Creates JWT tokens for principals

use anyhow::{bail, Result};
use colored::Colorize;

use coven_grpc::ChannelConfig;
use coven_proto::coven::{admin_service_client::AdminServiceClient, CreateTokenRequest};

use super::TokenCommand;
use crate::client::AuthInterceptor;

pub async fn run(gateway: &str, token: Option<&str>, cmd: TokenCommand) -> Result<()> {
    let Some(token) = token else {
        bail!("Authentication required. Set COVEN_TOKEN environment variable or use --token flag.");
    };

    match cmd {
        TokenCommand::Create { principal_id, ttl } => {
            create_token(gateway, token, principal_id, ttl).await
        }
    }
}

async fn create_token(
    gateway: &str,
    token: &str,
    principal_id: String,
    ttl_seconds: i64,
) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let request = CreateTokenRequest {
        principal_id: principal_id.clone(),
        ttl_seconds,
    };

    let response = client.create_token(request).await?;
    let token_response = response.into_inner();

    println!("{}", "Token created".green().bold());
    println!();
    println!("{}: {}", "Principal ID".dimmed(), principal_id);
    println!("{}: {}", "Expires".dimmed(), format_ttl(ttl_seconds));
    println!();
    println!("{}", "Token (save this now!):".yellow().bold());
    println!();
    println!("{}", token_response.token);
    println!();
    println!("{}", "This token will not be shown again.".red());

    Ok(())
}

fn format_ttl(seconds: i64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;

    if days > 0 {
        if hours > 0 {
            format!("{} days, {} hours", days, hours)
        } else {
            format!("{} days", days)
        }
    } else if hours > 0 {
        format!("{} hours", hours)
    } else {
        format!("{} seconds", seconds)
    }
}
