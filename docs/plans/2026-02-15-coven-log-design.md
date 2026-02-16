# coven-log: Holistic Logging Crate

**Date:** 2026-02-15
**Status:** Approved

## Problem

Logging across coven binaries is inconsistent:
- Each binary copy-pastes 5 lines of tracing-subscriber setup with slight variations
- Two binaries (coven-agent, coven-serve) don't initialize at all
- TUI needed special file-based handling but had no subscriber, causing logs to bleed onto the terminal
- Three different patterns exist: stderr, file-based, crate-filtered

## Design

A new Layer 0 crate (`crates/coven-log/`) with no internal coven dependencies. Three public functions covering all use cases:

### API

```rust
/// Standard logging to stderr. Default: INFO level, RUST_LOG override.
pub fn init();

/// File-based logging for TUI apps. Default: WARN level, RUST_LOG override.
/// Logs to ~/.config/coven/{app_name}/{app_name}.log
pub fn init_file(app_name: &str);

/// Crate-filtered logging to stderr. Default: INFO for named crate, WARN for everything else.
pub fn init_for(crate_name: &str);
```

All three functions handle errors internally with `eprintln!` — never panic, never return errors.

### Dependencies

`tracing`, `tracing-subscriber` (workspace), `dirs` (for config dir)

### Migration

| Binary | Current | New call |
|--------|---------|----------|
| coven-cli | 5-line inline init | `coven_log::init()` |
| coven-link | 5-line inline init | `coven_log::init()` |
| coven-admin | 5-line inline init | `coven_log::init()` |
| coven-agent | No init | `coven_log::init()` |
| coven-swarm | 5-line inline init | `coven_log::init()` |
| coven-tui-v2 | File init (just added) | `coven_log::init_file("tui")` |
| coven-matrix-rs | Crate-filtered init | `coven_log::init_for("coven_matrix_rs")` |
| coven-slack-rs | Crate-filtered init | `coven_log::init_for("coven_slack_rs")` |
| coven-telegram-rs | Crate-filtered init | `coven_log::init_for("coven_telegram_rs")` |

### Not Building (YAGNI)

- No log rotation
- No builder pattern
- No JSON output mode
- No runtime reconfiguration
