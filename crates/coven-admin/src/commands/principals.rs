// ABOUTME: Implementation of 'coven-admin principals' commands
// ABOUTME: Manages principals (agents, clients) in the gateway

use anyhow::{bail, Result};
use colored::Colorize;

use coven_grpc::ChannelConfig;
use coven_proto::coven::{
    admin_service_client::AdminServiceClient, CreatePrincipalRequest, DeletePrincipalRequest,
    ListPrincipalsRequest,
};

use super::PrincipalsCommand;
use crate::client::AuthInterceptor;

pub async fn run(gateway: &str, token: Option<&str>, cmd: PrincipalsCommand) -> Result<()> {
    let Some(token) = token else {
        bail!("Authentication required. Set COVEN_TOKEN environment variable or use --token flag.");
    };

    match cmd {
        PrincipalsCommand::List { r#type } => list_principals(gateway, token, r#type).await,
        PrincipalsCommand::Create {
            r#type,
            name,
            fingerprint,
            role,
        } => create_principal(gateway, token, r#type, name, fingerprint, role).await,
        PrincipalsCommand::Delete { id } => delete_principal(gateway, token, id).await,
    }
}

async fn list_principals(
    gateway: &str,
    token: &str,
    type_filter: Option<String>,
) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let response = client
        .list_principals(ListPrincipalsRequest {
            r#type: type_filter,
            status: None,
        })
        .await?;
    let principals = response.into_inner().principals;

    if principals.is_empty() {
        println!("{}", "No principals found".dimmed());
        return Ok(());
    }

    println!("{}", format!("Principals ({})", principals.len()).bold());
    println!();

    for p in principals {
        let status_colored = match p.status.as_str() {
            "approved" => p.status.green(),
            "pending" => p.status.yellow(),
            "revoked" => p.status.red(),
            _ => p.status.normal(),
        };

        let type_icon = if p.r#type == "agent" { "🤖" } else { "👤" };

        println!(
            "{} {} {}",
            type_icon,
            p.display_name.bold(),
            format!("({})", p.r#type).dimmed()
        );
        println!("    {}: {}", "ID".dimmed(), p.id);
        println!("    {}: {}", "Status".dimmed(), status_colored);
        if !p.roles.is_empty() {
            println!("    {}: {}", "Roles".dimmed(), p.roles.join(", "));
        }
        if let Some(ref fp) = p.pubkey_fp {
            if !fp.is_empty() {
                println!("    {}: {}", "Fingerprint".dimmed(), fp);
            }
        }
        println!();
    }

    Ok(())
}

async fn create_principal(
    gateway: &str,
    token: &str,
    principal_type: String,
    display_name: String,
    fingerprint: Option<String>,
    roles: Vec<String>,
) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let request = CreatePrincipalRequest {
        r#type: principal_type.clone(),
        display_name: display_name.clone(),
        pubkey: None,
        pubkey_fp: fingerprint,
        roles,
    };

    let response = client.create_principal(request).await?;
    let principal = response.into_inner();

    println!("{}", "Principal created".green().bold());
    println!("  {}: {}", "ID".dimmed(), principal.id);
    println!("  {}: {}", "Type".dimmed(), principal.r#type);
    println!("  {}: {}", "Name".dimmed(), principal.display_name);
    println!("  {}: {}", "Status".dimmed(), principal.status);
    if !principal.roles.is_empty() {
        println!("  {}: {}", "Roles".dimmed(), principal.roles.join(", "));
    }

    Ok(())
}

async fn delete_principal(gateway: &str, token: &str, id: String) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let request = DeletePrincipalRequest { id: id.clone() };
    client.delete_principal(request).await?;

    println!("{} {}", "Principal deleted:".green().bold(), id);

    Ok(())
}
