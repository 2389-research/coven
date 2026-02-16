// ABOUTME: Shared logging setup for all coven binaries
// ABOUTME: Three functions: init() for stderr, init_file() for TUI, init_for() for bridges

use tracing_subscriber::EnvFilter;

/// Standard logging to stderr. Default: INFO level, RUST_LOG override.
/// Used by CLI and daemon binaries.
pub fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}

/// File-based logging for TUI apps. Default: WARN level, RUST_LOG override.
/// Logs to ~/.config/coven/{app_name}/{app_name}.log
/// If setup fails, prints a warning to stderr and continues without logging.
pub fn init_file(app_name: &str) {
    if let Err(e) = init_file_inner(app_name) {
        eprintln!("Warning: failed to set up file logging: {e}");
    }
}

fn init_file_inner(app_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = dirs::config_dir().ok_or("could not determine config directory")?;
    let log_dir = config_dir.join("coven").join(app_name);
    std::fs::create_dir_all(&log_dir)?;

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(format!("{app_name}.log")))?;

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::WARN.into()))
        .with_ansi(false)
        .init();

    Ok(())
}

/// Crate-filtered logging to stderr. Default: INFO for named crate, WARN for everything else.
/// Used by bridge binaries (matrix, slack, telegram).
pub fn init_for(crate_name: &str) {
    let directive = format!("{crate_name}=info");
    let filter = EnvFilter::from_default_env()
        .add_directive(tracing::Level::WARN.into())
        .add_directive(
            directive
                .parse()
                .unwrap_or_else(|_| tracing::Level::INFO.into()),
        );

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_init() {
        let _ = super::init as fn();
    }

    #[test]
    fn exports_init_file() {
        let _ = super::init_file as fn(&str);
    }

    #[test]
    fn exports_init_for() {
        let _ = super::init_for as fn(&str);
    }
}
