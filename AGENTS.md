# AGENTS.md

> Keep this file current. Whenever the project changes in a way that would mislead a future session, update this file or the linked `docs/agents/*` file in the same change.

## What This Is

`fez` is an agent-native management CLI for Fedora/RHEL. It drives `cockpit-bridge` over the framed JSON protocol and reaches remote hosts through the system OpenSSH client.

- Rust edition 2021, MSRV **1.92**. Keep `rust-toolchain.toml` and `Cargo.toml` in sync on bumps.
- Crates.io package name is `rusty-fez`; the installed binary and library crate name remain `fez`.
- Thin binaries over a library. `src/lib.rs` owns the module tree and `run()` is the single entry point.
- `fez` holds no state. It never writes state files, caches, locks, spools, or temp snapshots. The only write sink is the append-only audit log, which fez never reads back.

## Progressive Docs

Read the narrow doc before changing that area:

- `docs/agents/release.md` - local commands, CI gates, MSRV, supply-chain checks, release, and packaging.
- `docs/agents/cli-contract.md` - clap entry points, envelopes, exit codes, capability descriptors, help/describe/guide/man, and safety globals.
- `docs/agents/capabilities.md` - bridge protocol architecture plus services, packages, PackageKit fallback, network, firewall, audit, rendering, and privilege escalation behavior.
- `docs/agents/testing.md` - integration scaffolding, fake bridge behavior, test-only env vars, compact JSON assertions, and E2E matrix notes.

## Commands

Use the Makefile, not raw cargo, for gated work. `make` defaults to `check`.

- `make check` - local one-shot gate: `lint test docs-coverage security`
- `make lint` - `cargo fmt --check` then `cargo clippy --all-targets -- -D warnings`
- `make test` - `cargo test` (unit + integration)
- `make docs-coverage` - requires nightly; enforces 100% public doc coverage
- `make security` - `cargo deny check` plus `cargo machete`
- `make msrv` - `cargo +1.92 check --all-targets`
- Single integration test: `cargo test --test services name_of_test`

For command details, CI caveats, release flow, and packaging, read `docs/agents/release.md`.

## Architecture Map

- `protocol/` - bridge framing, messages, request/reply calls, and signal collection.
- `transport/` - local bridge spawning and SSH transport.
- `capabilities/` - command implementations plus shared rendering.
- `capability/` - canonical machine-readable command descriptors.
- `dispatch.rs` - parsed CLI routing.
- `cli.rs` - clap definitions, registry-enriched command tree, usage rendering, guide, and man output.
- `envelope.rs` - compact `fez/v1` JSON response envelope.
- `error.rs` - stable error codes, exit codes, and structured error details.
- `safety.rs` - protected-operation guardrails.
- `audit.rs` - JSON-lines audit log sink.
- `mcp/` - MCP server over stdio.

## Non-Negotiable Contracts

- Error codes and exit codes are stable API. Update tests and `docs/agents/cli-contract.md` when changing `FezError`, `EXIT_CODES`, or usage rendering.
- `--json` output uses the compact `fez/v1` envelope. Tests should assert compact substrings like `"kind":"ServiceList"`.
- The capability registry in `src/capability/mod.rs` is canonical for help, examples, `fez describe`, and `fez man`. Keep clap args and descriptors in sync.
- Privileged operations escalate through cockpit, not fez-owned sudo/password handling. `FEZ_ESCALATION=off` disables escalation; any other value forces that single mechanism.
- Mutations apply to subsystem-owned state. If a feature seems to need memory across invocations, push that state into the subsystem or recompute it.
- CI actions are SHA-pinned. When bumping an action, pin the new commit SHA and keep the `# vX.Y.Z` comment accurate.

## Env Vars

Runtime knobs: `FEZ_BRIDGE`, `FEZ_AUDIT`, `FEZ_SSH_CONFIG`, `FEZ_ESCALATION`. Audit records also carry `FEZ_ACTOR`, `FEZ_CORRELATION_ID`, `FEZ_TARGET_HOST`, `FEZ_OPERATION`, `FEZ_UNIT`, and related metadata.

Test-only fake-bridge knobs live in `docs/agents/testing.md`.

## Conventions

- Error handling: `thiserror` enum plus `pub type Result<T>`, propagate with `?`. No `anyhow`.
- Keep async/IO thin; put pure logic in sync helpers. Unit tests inline in `#[cfg(test)] mod tests`.
- CLI serialization and bridge request bodies have different `None`/default semantics; test them separately.
- README links a missing design spec at `docs/superpowers/specs/2026-06-09-agentic-os-design.md`; do not trust that link until it exists.
