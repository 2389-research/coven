// ABOUTME: Implementation of 'coven-admin me' command
// ABOUTME: Shows authenticated principal information

use anyhow::{bail, Result};
use colored::Colorize;

use coven_grpc::ChannelConfig;
use coven_proto::coven::client_service_client::ClientServiceClient;

use crate::client::AuthInterceptor;

pub async fn run(gateway: &str, token: Option<&str>) -> Result<()> {
    let Some(token) = token else {
        bail!("Authentication required. Set COVEN_TOKEN environment variable or use --token flag.");
    };

    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = ClientServiceClient::with_interceptor(channel, interceptor);

    let response = client.get_me(()).await?;
    let me = response.into_inner();

    println!("{}", "Principal Info".bold());
    println!("  {}:        {}", "ID".dimmed(), me.principal_id);
    println!("  {}:      {}", "Type".dimmed(), me.principal_type);
    println!("  {}: {}", "Display Name".dimmed(), me.display_name);
    println!("  {}:    {}", "Status".dimmed(), format_status(&me.status));
    println!(
        "  {}:     {}",
        "Roles".dimmed(),
        if me.roles.is_empty() {
            "(none)".dimmed().to_string()
        } else {
            me.roles.join(", ")
        }
    );

    if let Some(member_id) = &me.member_id {
        println!();
        println!("{}", "Member Info".bold());
        println!("  {}:     {}", "Member ID".dimmed(), member_id);
        if let Some(member_name) = &me.member_display_name {
            println!("  {}: {}", "Member Name".dimmed(), member_name);
        }
    }

    Ok(())
}

fn format_status(status: &str) -> String {
    match status {
        "approved" | "online" => status.green().to_string(),
        "pending" => status.yellow().to_string(),
        "revoked" | "offline" => status.red().to_string(),
        _ => status.to_string(),
    }
}
