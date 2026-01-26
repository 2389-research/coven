// ABOUTME: Implementation of 'coven-admin agents' commands
// ABOUTME: Lists connected agents from the gateway

use anyhow::{bail, Result};
use colored::Colorize;

use coven_grpc::ChannelConfig;
use coven_proto::coven::{client_service_client::ClientServiceClient, ListAgentsRequest};

use super::AgentsCommand;
use crate::client::AuthInterceptor;

pub async fn run(gateway: &str, token: Option<&str>, cmd: AgentsCommand) -> Result<()> {
    let Some(token) = token else {
        bail!("Authentication required. Set COVEN_TOKEN environment variable or use --token flag.");
    };

    match cmd {
        AgentsCommand::List { workspace } => list_agents(gateway, token, workspace).await,
    }
}

async fn list_agents(gateway: &str, token: &str, workspace: Option<String>) -> Result<()> {
    let config = ChannelConfig::new(gateway).without_keep_alive();
    let channel = coven_grpc::create_channel(&config).await?;

    let interceptor = AuthInterceptor::new(Some(token.to_string()));
    let mut client = ClientServiceClient::with_interceptor(channel, interceptor);

    let request = ListAgentsRequest { workspace };
    let response = client.list_agents(request).await?;
    let agents = response.into_inner().agents;

    if agents.is_empty() {
        println!("{}", "No agents connected".dimmed());
        return Ok(());
    }

    println!("{}", format!("Connected Agents ({})", agents.len()).bold());
    println!();

    for agent in agents {
        let status = if agent.connected {
            "●".green()
        } else {
            "○".red()
        };

        println!(
            "{} {} {}",
            status,
            agent.name.bold(),
            format!("({})", agent.id).dimmed()
        );

        if !agent.backend.is_empty() {
            println!("    {}: {}", "Backend".dimmed(), agent.backend);
        }
        if !agent.working_dir.is_empty() {
            println!("    {}: {}", "Working Dir".dimmed(), agent.working_dir);
        }

        // Show metadata if available
        if let Some(metadata) = agent.metadata {
            if let Some(git) = metadata.git {
                let git_info = format!(
                    "{}{}",
                    git.branch,
                    if git.dirty { " (dirty)" } else { "" }
                );
                println!("    {}: {}", "Git".dimmed(), git_info);
            }
            if !metadata.workspaces.is_empty() {
                println!(
                    "    {}: {}",
                    "Workspaces".dimmed(),
                    metadata.workspaces.join(", ")
                );
            }
        }
        println!();
    }

    Ok(())
}
