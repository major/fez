# AGENTS.md

> Keep this file current. Whenever the project changes in a way that would
> mislead a future session (commands, gates, module boundaries, env vars, env
> setup, release flow, CLI surface, or help/example text), update AGENTS.md in
> the same change. This is a standing, high-priority directive, not a one-time
> task.

## What this is

`fez` is an agent-native management CLI for Fedora/RHEL. It drives
`cockpit-bridge` over its framed JSON protocol (spawns the bridge, opens
channels, delegates privilege escalation to it) and reaches remote hosts by
shelling out to the system OpenSSH client. Rust, edition 2021, MSRV **1.92**
(pinned in `rust-toolchain.toml` and `Cargo.toml`; keep both in sync on bump).

Crates.io package name is `rusty-fez` because `fez` is already taken. The
installed binary and library crate name stay `fez`.

## Commands

Use the Makefile, not raw cargo, for gated work. `make` defaults to `check`.

- `make check` - local one-shot gate: `lint test docs-coverage`
- `make lint` - `cargo fmt --check` then `cargo clippy --all-targets -- -D warnings`
- `make test` - `cargo test` (unit + integration)
- `make coverage` / `make coverage-html` - lcov for Codecov / HTML report
- `make docs-coverage` - **requires the nightly toolchain** (`cargo +nightly`); enforces 100% docstring coverage via `scripts/docs_coverage.py`
- Single test: `cargo test --test services name_of_test` (integration files are `tests/cli.rs`, `tests/mcp.rs`, `tests/services.rs`)

## Gates (CI: `.github/workflows/ci.yml`)

All enforced; do not regress:

- **90% project** + **95% patch** test coverage (Codecov, `codecov.yml`)
- **100% docstring coverage**. Crate root denies missing docs; every `pub` item needs a doc comment, fallible methods need an `# Errors` section.
- `clippy -D warnings` and `cargo fmt --check`. Never add `#![deny(warnings)]` to source; that lint pressure lives in CI flags only.

## Architecture

Thin binaries over a library (`src/lib.rs` owns the module tree; `run()` is the single entry point). Key modules:

- `protocol/` - wire protocol to the bridge: `frame` (framed JSON), `message`, `client`
- `transport/` - `local` (spawns the bridge) and `ssh` (OpenSSH client) ways to reach it
- `capabilities/` - the actual commands fez runs (e.g. `services`); `capability/` holds machine-readable descriptors of that surface
- `dispatch.rs` - routes parsed CLI to capabilities (private module)
- `cli.rs` - clap definitions plus `command()`/`parse()` (the registry-enriched command tree) and `raw_command()` (the bare derived tree). New top-level verbs beyond the capability commands: `fez guide` (agent contract; `--json` emits an `AgentGuide` envelope), `fez completions <shell>` (bash/zsh/fish), and hidden `fez man` (emits the `fez.1` roff page; packaging runs it during `%build`). `envelope.rs` - the `fez/v1` JSON response envelope
- `safety.rs` - guardrails: protected units refuse mutation without `--force`
- `audit.rs` - JSON-lines audit log of attempted + completed mutations (each executed mutation writes two records: attempt + result)
- `mcp/` - MCP server (`fez mcp`, JSON-RPC 2.0 over stdio)

Errors: single `FezError` enum (`src/error.rs`) with stable string `code()` and `exit_code()` mappings. Exit codes are part of the contract (e.g. protected-unit = 8, timeout = 5, bridge spawn/closed = 6, dbus = 7). E2E and integration tests assert on them; update tests when you touch the mapping.

## Testing quirks

- Integration tests drive a **fake bridge binary** (`src/bin/fake_bridge.rs`, built as `fez-fake-bridge`) instead of a real `cockpit-bridge`. Tests point fez at it via the `FEZ_BRIDGE` env var set to `env!("CARGO_BIN_EXE_fez-fake-bridge")`. The fake reports `chronyd` inactive and `sshd` active; assertions depend on that.
- E2E (`test/e2e/run.sh`) provisions a real cloud host via Terraform, installs `cockpit-bridge`, and exercises the real transport. Expensive and destructive (auto-`destroy` on exit). It auto-`tee`s every run to `test/e2e/logs/run-<ts>.log` (gitignored) with a `last-run.log` symlink; read the log on failure. It pins SSH config with `FEZ_SSH_CONFIG` (`ssh -F`), not `HOME`, because OpenSSH ignores `$HOME/.ssh/config` non-interactively.

## Env vars

Runtime knobs read from the environment: `FEZ_BRIDGE` (bridge binary path, used by tests), `FEZ_AUDIT` (audit sink, e.g. `file:/path/audit.jsonl`), `FEZ_SSH_CONFIG` (ssh `-F` config). Audit records also carry `FEZ_ACTOR`, `FEZ_CORRELATION_ID`, `FEZ_TARGET_HOST`, `FEZ_OPERATION`, `FEZ_UNIT`, etc.

## Conventions

- Error handling: `thiserror` enum + `pub type Result<T>`, propagate with `?`. No `anyhow`.
- Keep async/IO thin; put pure logic in sync helpers. Unit tests inline in `#[cfg(test)] mod tests`.
- `--json` output everywhere uses the `fez/v1` envelope; CLI serialization and bridge request bodies have different `None`/default semantics, so test them separately.
- CLI is self-documenting and stays usable by an LLM the way `git`/`gh` are. The capability registry in `src/capability/mod.rs` is **canonical**: `src/capability/help.rs::inject()` walks the derived clap tree and attaches each descriptor's `long`/`examples` as `long_about`/`after_help`, so `--help`, `fez describe`, `fez man`, and (indirectly) completions all derive from it. When you add or change a command, flag, or arg, update its clap `about` **and** the matching descriptor (summary, long, flags, examples) in the same change. The `services <action> <-> services.<action>` path/id mapping and the example wiring are locked by drift-guard tests in `tests/cli.rs`. `fez guide` renders the exit-code contract from `src/error.rs::EXIT_CODES` (one table, cross-checked against `exit_code()` by a test).

## Release

`release-plz` (`.github/workflows/release-plz.yml`, `cd.yml`, `release-plz.toml`) handles version bumps and changelog. Crates.io publishes use the `rusty-fez` package name; release binaries and RPMs still install the `fez` command. RPM packaging lives in `packaging/` (`fez.spec`, `make-vendor.sh`).

## Notes

- README links a design spec at `docs/superpowers/specs/2026-06-09-agentic-os-design.md` that is **not present** in the repo (`docs/superpowers/` only has the release-process and coverage-enforcement specs/plans). Do not trust that link.
