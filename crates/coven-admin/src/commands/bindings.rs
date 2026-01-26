// ABOUTME: Implementation of 'coven-admin bindings' commands
// ABOUTME: Manages bindings between frontends/channels and agents

use anyhow::{bail, Result};
use colored::Colorize;

use coven_grpc::ChannelConfig;
use coven_proto::coven::{
    admin_service_client::AdminServiceClient, CreateBindingRequest, DeleteBindingRequest,
    ListBindingsRequest,
};

use super::BindingsCommand;
use crate::client::AuthInterceptor;

pub async fn run(gateway: &str, token: Option<&str>, cmd: BindingsCommand) -> Result<()> {
    let Some(token) = token else {
        bail!("Authentication required. Set COVEN_TOKEN environment variable or use --token flag.");
    };

    match cmd {
        BindingsCommand::List => list_bindings(gateway, token).await,
        BindingsCommand::Create {
            frontend,
            channel_id,
            agent_id,
        } => create_binding(gateway, token, frontend, channel_id, agent_id).await,
        BindingsCommand::Delete { id } => delete_binding(gateway, token, id).await,
    }
}

async fn list_bindings(gateway: &str, token: &str) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let response = client
        .list_bindings(ListBindingsRequest {
            frontend: None,
            agent_id: None,
        })
        .await?;
    let bindings = response.into_inner().bindings;

    if bindings.is_empty() {
        println!("{}", "No bindings configured".dimmed());
        return Ok(());
    }

    println!("{}", format!("Bindings ({})", bindings.len()).bold());
    println!();

    for binding in bindings {
        println!(
            "{} {} → {}",
            "●".cyan(),
            format!("{}:{}", binding.frontend, binding.channel_id).bold(),
            binding.agent_id.green()
        );
        println!("    {}: {}", "ID".dimmed(), binding.id);
        if !binding.created_at.is_empty() {
            println!("    {}: {}", "Created".dimmed(), binding.created_at);
        }
        println!();
    }

    Ok(())
}

async fn create_binding(
    gateway: &str,
    token: &str,
    frontend: String,
    channel_id: String,
    agent_id: String,
) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let request = CreateBindingRequest {
        frontend: frontend.clone(),
        channel_id: channel_id.clone(),
        agent_id: agent_id.clone(),
    };

    let response = client.create_binding(request).await?;
    let binding = response.into_inner();

    println!("{}", "Binding created".green().bold());
    println!("  {}: {}", "ID".dimmed(), binding.id);
    println!(
        "  {}: {}:{}",
        "Route".dimmed(),
        binding.frontend,
        binding.channel_id
    );
    println!("  {}: {}", "Agent".dimmed(), binding.agent_id);

    Ok(())
}

async fn delete_binding(gateway: &str, token: &str, id: String) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = AdminServiceClient::with_interceptor(channel, interceptor);

    let request = DeleteBindingRequest { id: id.clone() };
    client.delete_binding(request).await?;

    println!("{} {}", "Binding deleted:".green().bold(), id);

    Ok(())
}
