# Rust code smells in `fez`

Ranked by the number of source lines the smell affects. Each was found by
reading the capability modules and grepping for repeated shapes; `cargo clippy
--all-targets` is already clean, so these are structural, not lint-level.

## 1. Long Method — `firewall::mutate` (≈241 lines)

`src/capabilities/firewall.rs`

`mutate` is a single function with a ten-arm `match` over every firewall
mutation (add/remove service, add/remove port, set-default-zone, reload,
confirm, panic, masquerade). It is the largest function in the codebase, and
the file it lives in (1774 lines) is the largest module. Each arm mixes zone
resolution, safety gating, the audited bridge call, and view construction, so
the reader has to scroll the whole body to find one operation and the arms
cannot be unit-tested in isolation.

**Affected:** ~241 lines (the function body).

**Fix:** split each mutation branch into a focused `mutate_*` helper that
returns `Result<View>`; `mutate` becomes a thin dispatcher that opens the
channel and routes. Behaviour is unchanged and the existing
`tests/firewall.rs` integration suite plus inline unit tests cover it.

## 2. Duplicated render logic — package `plan_view` (≈70 lines, ~35 duplicated)

`src/capabilities/packages.rs` and `src/capabilities/packages_pk.rs`

The dnf5daemon backend and the PackageKit fallback each define their own
`plan_view`. The `kind` selection (`PackagePlan` vs `PackageMutation`) and the
entire dry-run / applied human string (`"... would install N, remove N,
upgrade N, downgrade N package(s)"`) are copied verbatim between the two
files. Any wording or count change has to be made in two places and they can
silently drift.

**Affected:** ~70 lines across two functions, ~35 of them identical.

**Fix:** extract `plan_kind(dry_run)` and `plan_human(verb, specs, host,
counts, dry_run)` shared helpers in the `packages` module and call them from
both backends. The two `*_view` builders keep their backend-specific data
payloads.

## 3. Duplicated bridge bootstrap + error mapping (≈30 lines)

`src/capabilities/{firewall,network,packages,services}.rs`

Two copy-pasted shapes:

- The bridge bootstrap triple
  ```rust
  let transport = transport::from_host(cli.host.as_deref());
  let mut client = BridgeClient::connect(transport.as_ref())?;
  let host = client.host().to_string();
  ```
  appears at every capability entry point (6 connect sites across 4 files).
- The "the daemon isn't there" mapping
  ```rust
  Err(FezError::Dbus { name, .. }) if is_service_unknown(&name) => ...
  ```
  is repeated four times (three in `packages.rs`, one in `firewall.rs`).

**Affected:** ~30 lines across four files.

**Fix:** add a `capabilities::connect(cli)` helper for the bootstrap and a
`map_service_unknown` combinator that maps a `is_service_unknown` `Dbus` error
to a caller-supplied dependency-missing error, leaving other errors untouched.

---

### Deliberately out of scope

- **Stringly-typed `serde_json::Value` access at the bridge boundary** (120+
  index/`as_*` sites). This is the single largest cross-cutting smell, but a
  per-capability typed-model migration is already in flight on other branches
  (`network-typed-boundary`, `services-typed-boundary`, `package-output-models`,
  …). Rewriting it here would duplicate and conflict with that work.
- **`firewall.rs` god module (1774 lines).** Splitting it is a large, risky
  move better done on its own; smell #1 takes the highest-value bite out of it.
