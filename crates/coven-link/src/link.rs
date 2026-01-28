// ABOUTME: Core linking logic for coven-link
// ABOUTME: Handles key generation, code request, polling, and config setup

use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::Deserialize;
use std::time::Duration;

use crate::config::CovenConfig;

#[derive(Deserialize)]
struct LinkRequestResponse {
    code: String,
    expires_at: String,
}

#[derive(Deserialize)]
struct LinkStatusResponse {
    status: String,
    token: Option<String>,
    principal_id: Option<String>,
}

pub async fn run(gateway: String, name: Option<String>, key_path: Option<String>) -> Result<()> {
    // Check if already configured
    if CovenConfig::exists() {
        println!(
            "{} Device already linked. Config at: {}",
            "!".yellow().bold(),
            CovenConfig::config_path()?.display()
        );
        println!("  To re-link, remove the config file first.");
        return Ok(());
    }

    // Determine device name
    let device_name = name.unwrap_or_else(|| {
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    });

    println!("{}", "Coven Device Linking".bold());
    println!();

    // Load or generate SSH key
    let key_path = match key_path {
        Some(p) => std::path::PathBuf::from(p),
        None => CovenConfig::key_path().context("Failed to determine key path")?,
    };

    println!(
        "{} Loading SSH key from {}...",
        "[1/4]".dimmed(),
        key_path.display()
    );

    let private_key = coven_ssh::load_or_generate_key(&key_path)
        .context("Failed to load or generate SSH key")?;
    let fingerprint = coven_ssh::compute_fingerprint(private_key.public_key())
        .context("Failed to compute key fingerprint")?;

    println!("  Fingerprint: {}", fingerprint.dimmed());

    // Normalize gateway URL
    let gateway_http = normalize_gateway_url(&gateway);
    let gateway_grpc = derive_grpc_address(&gateway);

    println!(
        "{} Requesting link code from {}...",
        "[2/4]".dimmed(),
        gateway_http
    );

    // Request link code (with timeout)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client")?;
    let resp = client
        .post(format!("{}/admin/api/link/request", gateway_http))
        .json(&serde_json::json!({
            "fingerprint": fingerprint,
            "device_name": device_name,
        }))
        .send()
        .await
        .context("Failed to connect to gateway")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Gateway returned error {}: {}", status, body);
    }

    let link_resp: LinkRequestResponse = resp.json().await.context("Failed to parse response")?;

    println!();
    println!("{}", "━".repeat(50).dimmed());
    println!();
    println!(
        "  Enter this code in the gateway web UI at {}",
        format!("{}/admin/link", gateway_http).cyan()
    );
    println!();
    println!(
        "  {}",
        format!("  {}  ", link_resp.code)
            .on_white()
            .black()
            .bold()
    );
    println!();
    println!("  Code expires at: {}", link_resp.expires_at.dimmed());
    println!();
    println!("{}", "━".repeat(50).dimmed());
    println!();

    println!("{} Waiting for approval...", "[3/4]".dimmed());

    // Poll for approval
    let token;
    let principal_id;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let resp = client
            .get(format!(
                "{}/admin/api/link/status/{}",
                gateway_http, link_resp.code
            ))
            .send()
            .await
            .context("Failed to check status")?;

        if !resp.status().is_success() {
            bail!("Failed to check status: {}", resp.status());
        }

        let status: LinkStatusResponse = resp.json().await?;

        match status.status.as_str() {
            "approved" => {
                token = status.token.context("No token in approved response")?;
                principal_id = status
                    .principal_id
                    .context("No principal_id in approved response")?;
                break;
            }
            "expired" => {
                bail!("Link code expired. Please try again.");
            }
            "pending" => {
                print!(".");
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            other => {
                bail!("Unexpected status: {}", other);
            }
        }
    }

    println!();
    println!("  {}", "Approved!".green().bold());
    println!();

    // Save configuration
    println!("{} Saving configuration...", "[4/4]".dimmed());

    let config = CovenConfig {
        gateway: gateway_grpc,
        token,
        principal_id,
        device_name: device_name.clone(),
    };
    config.save().context("Failed to save configuration")?;

    println!();
    println!("{}", "Device linked successfully!".green().bold());
    println!();
    println!(
        "  Config saved to: {}",
        CovenConfig::config_path()?.display()
    );
    println!(
        "  Token saved to:  {}",
        CovenConfig::config_dir()?.join("token").display()
    );
    println!("  SSH key at:      {}", key_path.display());
    println!();
    println!("You can now use:");
    println!(
        "  {} - Connect this device as an agent",
        "coven agent run".cyan()
    );
    println!("  {} - Terminal UI", "coven chat".cyan());
    println!("  {} - Admin commands", "coven admin".cyan());
    println!();

    Ok(())
}

/// Normalizes a gateway URL to HTTP(S) base URL
fn normalize_gateway_url(gateway: &str) -> String {
    let url = gateway.trim_end_matches('/');

    // If it's already a full URL, use it
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }

    // If it looks like host:port, add http://
    if url.contains(':') {
        return format!("http://{}", url);
    }

    // Otherwise assume https
    format!("https://{}", url)
}

/// Derives gRPC address from gateway URL
fn derive_grpc_address(gateway: &str) -> String {
    // Default to http for gRPC (TLS usually handled at network layer e.g. Tailscale)
    // Only use https if gateway URL explicitly starts with https://
    let scheme = if gateway.starts_with("https://") {
        "https://"
    } else {
        "http://"
    };

    let url = gateway
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');

    // Extract hostname (strip port if present)
    // Handle IPv6 addresses like [::1]:443
    let hostname = if url.starts_with('[') {
        // IPv6 address: [::1]:443 or [::1]
        if let Some(bracket_end) = url.find(']') {
            &url[..=bracket_end]
        } else {
            url
        }
    } else if let Some(idx) = url.rfind(':') {
        // IPv4 or hostname with port
        &url[..idx]
    } else {
        url
    };

    format!("{}{}:50051", scheme, hostname)
}
