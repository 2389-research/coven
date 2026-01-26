// ABOUTME: fold-agent library exports
// ABOUTME: Re-exports client, wizard, and utility modules

pub mod client;
pub mod metadata;
pub mod pack_tool;
pub mod run;
pub mod single;
pub mod tui;
pub mod wizard;

// Re-export main entry points for convenience
pub use run::{run_agent, run_wizard, AgentRunConfig};

/// Build MCP URL with token appended as a path segment.
/// e.g., "http://localhost:8080/mcp" + "abc123" → "http://localhost:8080/mcp/abc123"
/// Token is percent-encoded to prevent path traversal or query/fragment injection.
pub fn build_mcp_url(endpoint: &str, token: &str) -> String {
    if let Ok(mut url) = url::Url::parse(endpoint) {
        let ok = url
            .path_segments_mut()
            .map(|mut seg| {
                seg.pop_if_empty().push(token);
            })
            .is_ok();
        if ok {
            return url.to_string();
        }
    }
    // Fallback for malformed or non-hierarchical URLs.
    // Insert token before the earliest query or fragment delimiter.
    let base = endpoint.trim_end_matches('/');
    let path_end = [base.find('?'), base.find('#')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(base.len());
    let path_part = base[..path_end].trim_end_matches('/');
    let encoded_token =
        percent_encoding::utf8_percent_encode(token, percent_encoding::NON_ALPHANUMERIC);
    format!("{}/{}{}", path_part, encoded_token, &base[path_end..])
}
