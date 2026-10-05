# Contributing to coven

Thanks for looking at coven. This document says what we want from a bug report
and from a patch, so you don't have to guess.

## Project status

coven is **stable and in use, but not under active feature development.** It
is maintained by the team at [2389 Research](https://github.com/2389-research),
primarily in service of projects built on top of it.

What that means for you, concretely:

- Bug reports are welcome and read, but a reply may take a while.
- Small, well-tested fixes are the contributions most likely to land.
- Large features are unlikely to be merged unless they've been discussed in an
  issue first. Please don't build one on spec.

If an issue has been open for a while without a response, that's a maintainer
backlog, not a rejection. Feel free to say so in the thread.

## Reporting a bug

Open an issue. A good coven bug report carries four things:

1. **Which crate.** coven is a 24-crate workspace — `coven-agent` failing to
   connect and `coven-tui-v2` rendering wrong are different problems. Name the
   crate or binary if you can, and don't worry about getting it right.
2. **A repro.** The exact commands you ran, in order. If it needs a gateway,
   say which gateway and how you started it.
3. **What you expected and what happened.** Include the error verbatim — not a
   paraphrase. Logs beat descriptions.
4. **Commit or version.** `git rev-parse --short HEAD` is fine. Also mention
   your OS and `rustc --version` if the build itself is what broke.

Deadlocks, hangs and anything timing-dependent are the bugs we most want in
writing, because they're the ones that don't reproduce on a maintainer's
machine. If you have a stack dump or a `RUST_LOG=debug` trace, attach it.

### If you already know the fix

Say so in the issue, and tell us **which kind of fix it is**:

- **A simple, contained fix** — open a PR. Include a test. See below.
- **A deeper fix that changes a boundary** — a trait, a protocol message, the
  crate dependency graph, a public API — please *describe* it in the issue
  rather than building it. Explain what's actually wrong and what the right
  shape would be, and let a maintainer pick between the cheap fix and the
  correct one. That decision depends on things outside the repo, and we'd
  rather you didn't spend a weekend on a PR we have to turn down.

Both are useful. One issue carrying "here's the small fix, and here's the
better fix I'd do if it were mine" is the single most useful thing you can
file.

## Development setup

Prerequisites:

- Rust 1.75 or newer
- `protoc` (CI pins the 25.x series)
- Go 1.21+ only if you need the gateway, which lives in a separate repo
- An Anthropic API key for anything that actually talks to a model

```bash
git clone https://github.com/2389-research/coven.git
cd coven
make            # check + test + clippy — the full local gate
```

The repo ships a [pre-commit](https://pre-commit.com) config that runs
`cargo fmt --check` and `cargo check` plus some whitespace and syntax checks.
Install the hook once and it keeps you off the obvious CI failures:

```bash
prek install    # or: pre-commit install
```

Other targets you'll want (`Makefile` has the rest):

```bash
make build      # debug build, whole workspace
make test       # cargo test --workspace
make clippy     # cargo clippy --workspace --all-targets -- -D warnings
make fmt        # format in place
make fmt-check  # check formatting without modifying
```

Running things locally — the gateway comes from `../coven-gateway`:

```bash
./target/debug/coven-agent run --name my-agent --working-dir ~/projects/myproject
./target/debug/coven-swarm supervisor
make run ARGS="..."        # or run-agent / run-swarm / run-chat
```

End-to-end tests run gateway, agents and scenarios in Docker. They need an API
key and they cost real tokens, so they aren't part of `make`:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
cd e2e && ./run-tests.sh          # --rebuild, --keep-running also available
```

See [`e2e/README.md`](e2e/README.md) for what the suite covers.

## Submitting a pull request

Branch off `main` and open the PR against `main`. Before you push, run `make` —
it runs the same three gates CI does.

CI will run four jobs, all of which must pass:

| Job | Command |
|---|---|
| Check | `cargo check --workspace --all-targets` |
| Test | `cargo test --workspace` |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format | `cargo fmt --all -- --check` |

Note that clippy runs with `-D warnings`: a lint is a build failure here. Don't
paper over one with `#[allow(...)]` unless you explain why in the PR.

What we look for in the diff:

- **A test that fails without your change.** For a bug fix, the test should
  encode the bug. If the bug genuinely can't be tested — a race, a TTY
  interaction — say so in the PR and describe how you verified it by hand.
- **The smallest change that fixes the thing.** Unrelated cleanups in the same
  diff make a fix harder to review and harder to revert. Open a second PR.
- **Respect for the layering.** Crates form an acyclic graph documented in
  [`CLAUDE.md`](CLAUDE.md); a change that adds a cycle or makes a lower layer
  depend on a higher one needs a conversation first.
- **Workspace dependencies.** External crates are declared once in the root
  `Cargo.toml` under `[workspace.dependencies]` and referenced as
  `foo.workspace = true`. Don't pin a version in a member crate.
- **`ABOUTME:` comments** at the top of every new source file — two lines
  saying what the file does. Existing files show the pattern.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)
with a crate-ish scope, matching the existing history:

```
fix(agent): send Register before awaiting agent_stream
feat(human): integrate coven-human into CLI with full TUI implementation
refactor: migrate bridge binaries to coven-log
docs: add coven-log design for holistic logging
```

Don't squash unrelated work into one commit, and don't bypass the pre-commit
hooks with `--no-verify`. If a hook fails, the hook is the message.

## Questions that aren't bugs

Discussions aren't enabled on this repo, so open an issue — "I can't tell
whether this is a bug or my misunderstanding" is a perfectly good opening line,
and the answer often turns into a docs fix.

## License

coven is MIT licensed. By contributing, you agree your contribution is licensed
the same way.
