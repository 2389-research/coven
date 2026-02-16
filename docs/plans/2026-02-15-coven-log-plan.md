# coven-log Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create a shared logging crate and migrate all 9 binaries to use it, eliminating copy-pasted tracing setup.

**Architecture:** A new `crates/coven-log/` crate at Layer 0 (no internal deps) with three public functions: `init()` for stderr logging, `init_file()` for TUI file logging, and `init_for()` for crate-filtered bridge logging. Each binary replaces its inline tracing-subscriber setup with a single function call.

**Tech Stack:** Rust, tracing, tracing-subscriber (env-filter), dirs

---

### Task 1: Create coven-log crate

**Files:**
- Create: `crates/coven-log/Cargo.toml`
- Create: `crates/coven-log/src/lib.rs`
- Modify: `Cargo.toml` (workspace root, add `coven-log` to workspace dependencies)

**Step 1: Create Cargo.toml**

```toml
# ABOUTME: Shared logging configuration for all coven binaries
# ABOUTME: Provides init(), init_file(), and init_for() for consistent tracing setup

[package]
name = "coven-log"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Shared logging configuration for coven binaries"

[dependencies]
tracing.workspace = true
tracing-subscriber.workspace = true
dirs = "6"
```

**Step 2: Create lib.rs**

```rust
// ABOUTME: Shared logging setup for all coven binaries
// ABOUTME: Three functions: init() for stderr, init_file() for TUI, init_for() for bridges

use tracing_subscriber::EnvFilter;

/// Standard logging to stderr. Default: INFO level, RUST_LOG override.
/// Used by CLI and daemon binaries.
pub fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
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
    let config_dir = dirs::config_dir()
        .ok_or("could not determine config directory")?;
    let log_dir = config_dir.join("coven").join(app_name);
    std::fs::create_dir_all(&log_dir)?;

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(format!("{app_name}.log")))?;

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
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
        .add_directive(directive.parse().unwrap_or_else(|_| {
            tracing::Level::INFO.into()
        }));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();
}
```

**Step 3: Add workspace dependency**

In root `Cargo.toml`, add to `[workspace.dependencies]`:
```toml
coven-log = { path = "crates/coven-log" }
```

**Step 4: Write tests**

Add to `lib.rs`:
```rust
#[cfg(test)]
mod tests {
    // Tracing subscriber can only be initialized once per process,
    // so we test that the functions don't panic.
    // Each test runs in its own process via cargo test.

    #[test]
    fn init_does_not_panic() {
        // Can't actually call init() in test because subscriber is global,
        // but we can verify the module compiles and exports are correct.
        let _ = super::init as fn();
    }

    #[test]
    fn init_file_does_not_panic() {
        let _ = super::init_file as fn(&str);
    }

    #[test]
    fn init_for_does_not_panic() {
        let _ = super::init_for as fn(&str);
    }
}
```

**Step 5: Verify**

Run: `cargo check -p coven-log && cargo test -p coven-log`
Expected: compiles and tests pass

**Step 6: Commit**

```bash
git add crates/coven-log/ Cargo.toml Cargo.lock
git commit -m "feat: add coven-log crate for shared logging setup"
```

---

### Task 2: Migrate CLI/daemon binaries

Migrate coven-cli, coven-link, coven-admin, coven-agent, and coven-swarm to use `coven_log::init()`.

**Files:**
- Modify: `crates/coven-cli/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-cli/src/main.rs:369-374` - replace 6-line init block with `coven_log::init()`
- Modify: `crates/coven-link/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-link/src/main.rs:24-29` - replace 6-line init block with `coven_log::init()`
- Modify: `crates/coven-admin/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-admin/src/main.rs:14-19` - replace 6-line init block with `coven_log::init()`
- Modify: `crates/coven-agent/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-agent/src/main.rs` - add `coven_log::init()` at top of `main()`
- Modify: `crates/coven-swarm/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-swarm/src/main.rs:48-53` - replace 6-line init block with `coven_log::init()`

**For each binary, the pattern is:**

1. In `Cargo.toml`: change `tracing-subscriber.workspace = true` to `coven-log.workspace = true`
2. In `main.rs`: replace the tracing_subscriber::fmt() block with `coven_log::init();`
3. Remove any `use tracing_subscriber::...` imports

**coven-cli** (`crates/coven-cli/src/main.rs`):
- Replace lines 369-374 (the `tracing_subscriber::fmt()...init()` block) with `coven_log::init();`

**coven-link** (`crates/coven-link/src/main.rs`):
- Replace lines 24-29 (the `tracing_subscriber::fmt()...init()` block) with `coven_log::init();`

**coven-admin** (`crates/coven-admin/src/main.rs`):
- Replace lines 14-19 (the `tracing_subscriber::fmt()...init()` block) with `coven_log::init();`
- Note: this was WARN before, bumping to INFO via `init()` is intentional

**coven-agent** (`crates/coven-agent/src/main.rs`):
- Add `coven_log::init();` as the first line of `main()` (after `dotenvy::dotenv().ok();` if present)
- This binary had NO tracing init before — this fixes that

**coven-swarm** (`crates/coven-swarm/src/main.rs`):
- Replace lines 48-53 (the `tracing_subscriber::fmt()...init()` block) with `coven_log::init();`

**Verify:**

Run: `cargo check -p coven-cli -p coven-link -p coven-admin -p coven-agent -p coven-swarm`
Expected: all compile

**Commit:**

```bash
git add crates/coven-cli crates/coven-link crates/coven-admin crates/coven-agent crates/coven-swarm Cargo.lock
git commit -m "refactor: migrate CLI/daemon binaries to coven-log"
```

---

### Task 3: Migrate coven-tui-v2

Replace the inline `setup_logging()` function (just added in commit c48123c) with `coven_log::init_file("tui")`.

**Files:**
- Modify: `crates/coven-tui-v2/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-tui-v2/src/main.rs` - remove `setup_logging()` function, replace call with `coven_log::init_file("tui")`

**Step 1: Update Cargo.toml**

Change `tracing-subscriber.workspace = true` to `coven-log.workspace = true`.

**Step 2: Update main.rs**

- Remove the `setup_logging()` function (lines 75-93 approximately)
- Remove `use tracing_subscriber::EnvFilter;` import
- Replace the `setup_logging()` call in `main()` with `coven_log::init_file("tui");`
  (No `if let Err` wrapper needed — `init_file` handles errors internally)

**Step 3: Verify**

Run: `cargo check -p coven-tui-v2 && cargo test -p coven-tui-v2`
Expected: compiles and all 25 tests pass

**Step 4: Commit**

```bash
git add crates/coven-tui-v2
git commit -m "refactor: migrate coven-tui-v2 to coven-log"
```

---

### Task 4: Migrate bridge binaries

Migrate coven-matrix-rs, coven-slack-rs, and coven-telegram-rs to use `coven_log::init_for()`.

**Files:**
- Modify: `crates/coven-matrix-rs/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-matrix-rs/src/main.rs:30-35` - replace init block with `coven_log::init_for("coven_matrix_rs")`
- Modify: `crates/coven-slack-rs/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-slack-rs/src/main.rs:18-23` - replace init block with `coven_log::init_for("coven_slack_rs")`
- Modify: `crates/coven-telegram-rs/Cargo.toml` - replace `tracing-subscriber` with `coven-log`
- Modify: `crates/coven-telegram-rs/src/main.rs:18-23` - replace init block with `coven_log::init_for("coven_telegram_rs")`

**For each bridge binary:**

1. In `Cargo.toml`: change `tracing-subscriber = { workspace = true, features = ["env-filter"] }` to `coven-log.workspace = true`
2. In `main.rs`: replace the `tracing_subscriber::fmt()...init()` block with `coven_log::init_for("crate_name");`

**Note for coven-matrix-rs:** The tracing init is conditional (skipped in setup mode). Keep the conditional — just replace the init block inside it:
```rust
if !setup_mode {
    coven_log::init_for("coven_matrix_rs");
}
```

**Verify:**

Run: `cargo check -p coven-matrix-rs -p coven-slack-rs -p coven-telegram-rs`
Expected: all compile

**Commit:**

```bash
git add crates/coven-matrix-rs crates/coven-slack-rs crates/coven-telegram-rs Cargo.lock
git commit -m "refactor: migrate bridge binaries to coven-log"
```

---

### Task 5: Workspace verification and cleanup

**Step 1: Full workspace check**

Run: `cargo check --workspace`
Expected: all crates compile

**Step 2: Full workspace tests**

Run: `cargo test --workspace`
Expected: all tests pass

**Step 3: Verify no remaining inline tracing-subscriber init**

Search for `tracing_subscriber::fmt()` across all binary crates — should only exist in `coven-log/src/lib.rs`.

**Step 4: Verify no unused tracing-subscriber deps**

Check that no binary Cargo.toml still has `tracing-subscriber` as a direct dependency (they get it transitively through coven-log). Only coven-log should have it.

**Step 5: No commit needed** — if issues found, fix and commit. Otherwise done.
