# Rust Code Smells — fez

Ranked by lines of code affected (most lines = #1). Measured against production
code (`#[cfg(test)]` blocks excluded except where the smell *is* test scaffolding).
Production total: ~8,941 lines across 32 files.

Method: brace-depth function sizing, prod/test split at `#[cfg(test)]`, and
`cargo clippy -W pedantic`. Counts are reproducible from the commands in each
section.

---

## #1 — God modules (~3,172 lines affected)

**Problem.** Three modules dominate the codebase and each mixes many concerns
in one file:

| File | Prod lines | Concerns crammed together |
|------|-----------|---------------------------|
| `src/capabilities/firewall.rs` | 1,377 | dispatch, hints, 92 functions: reads, mutations, audit wiring, zone/port parsing, drift, rendering |
| `src/bin/fake_bridge.rs` | 994 | every faked D-Bus surface (systemd, NetworkManager, firewalld, PackageKit, dnf5daemon) in one binary |
| `src/capability/mod.rs` | 801 | descriptor registry + two 160+ line schema builders + render helpers |

A 1,377-line module with 92 functions is hard to navigate, review, and test in
isolation. `firewall.rs` alone is 15% of production code.

**Fix.**
- Split `firewall.rs` into a `capabilities/firewall/` directory: `mod.rs`
  (dispatch + hints), `reads.rs` (status/list/show), `mutations.rs` (the
  `mutate_*` family + `run_audited`), `zone.rs` (parsing/effective-zone/drift).
  The existing `AuditedFirewallCall` / `run_audited` factoring stays — just
  relocate it.
- Split `fake_bridge.rs` into per-service reply modules (`nm.rs`, `fw.rs`,
  `pk.rs`, `systemd.rs`) behind a thin `main` dispatcher. This is the test
  harness, so churn risk is contained to integration tests.
- Move the schema builders out of `capability/mod.rs` (see #3).

**Acceptance criteria.**
- No production `.rs` file exceeds 600 lines (prod-only count).
- `make check` passes unchanged (lint + test + docs-coverage + security).
- No public API surface changes; `src/lib.rs` re-exports stay identical.
- Integration tests pass without modification to assertions.

---

## #2 — Over-long functions (~1,094 lines affected, 8 functions)

**Problem.** Eight functions exceed 70 lines; five exceed 100. Clippy
`too_many_lines` (pedantic) already flags the worst.

| Lines | Location | Function |
|-------|----------|----------|
| 276 | `src/bin/fake_bridge.rs:718` | `main` |
| 170 | `src/capability/mod.rs:351` | `output_schema` |
| 166 | `src/capability/mod.rs:567` | `flag_schema` |
| 122 | `src/bin/fake_bridge.rs:595` | `pk_emit` |
| 119 | `src/bin/fake_bridge.rs:374` | `fw_reply` |
| 94 | `src/capabilities/firewall.rs:1455` | `classify_routes_..._typed_plans` (test) |
| 75 | `src/bin/fake_bridge.rs:197` | `nm_reply` |
| 72 | `src/dispatch.rs:7` | `run` |

`output_schema` and `flag_schema` are giant `match` statements over inline data
(see #3). `main`/`*_reply`/`pk_emit` are long because each branch builds a reply
inline. `dispatch::run` is a long routing `match`.

**Fix.**
- For data-driven functions (`output_schema`, `flag_schema`), the fix is the
  same as #3 — move the table data out, leaving a thin lookup.
- For `fake_bridge` functions, extract each protocol branch into its own
  builder fn (pairs with the file split in #1).
- `dispatch::run` is an acceptable routing match; if split, group by capability
  into per-capability `dispatch` calls (most already exist).

**Acceptance criteria.**
- `cargo clippy --all-targets -- -W clippy::pedantic` reports zero
  `too_many_lines` warnings in production modules (test fns may use a scoped
  `#[allow]` with a comment).
- No production function exceeds 100 lines.
- Behavior unchanged: existing unit + integration tests pass.

---

## #3 — Static metadata hardcoded inside match arms (~336 lines affected)

**Problem.** `output_schema` (170 lines) and `flag_schema` (166 lines) in
`src/capability/mod.rs` are large `match kind { ... }` / `match flag { ... }`
blocks where each arm hand-writes a schema or a 6-tuple of flag attributes:

```rust
"--host" => ("string", "Target host. Defaults to localhost.", false,
             Some("localhost"), None, vec![]),
```

The rust-dev convention is explicit: *"For large static CLI metadata, keep
lookup/validation modules thin and move table data into private domain/table
modules."* This is metadata masquerading as control flow. Adding a flag or
output kind means editing a 166-line function instead of adding a row.

**Fix.**
- Define a `FlagSchema` table: `&[(&str, FlagSpec)]` or a small `const` array in
  a private `capability/flags.rs`. `flag_schema` becomes a lookup over the table.
- Likewise move output-kind schemas into a `capability/schemas.rs` table keyed
  by kind. The match collapses to `TABLE.iter().find(...)` plus the
  `object_schema`/`table_schema` helper calls that already exist.
- Keep the helper constructors (`string_prop`, `object_schema`, `array_of`) —
  they are the right abstraction; only the dispatch should be data, not code.

**Acceptance criteria.**
- `output_schema` and `flag_schema` each under 30 lines (lookup + fallback).
- The capability registry stays canonical: `fez describe`, `fez man`, help, and
  completions produce byte-identical output (assert with existing snapshot/
  substring tests).
- Adding a new flag requires only a new table row, no function-body edit
  (demonstrate with one added flag in a test, then revert).

---

## #4 — Duplicated reply-builder boilerplate in the fake bridge (~290 lines affected)

**Problem.** `fake_bridge.rs` has 73 `=>` match arms and repeated reply-frame
construction. The most repeated literal block (7×) is the `"/".to_string()`
property-path filler; `nm_reply`/`fw_reply`/`pk_emit` each rebuild reply frames
with near-identical envelope scaffolding around different payloads. Clippy also
flags `needless_pass_by_value` on `send_signal`, `argv_for`,
`argv_for_with_host` — `Value` passed by value but only read.

**Fix.**
- Extract a `reply(id, payload) -> Frame` / `error_reply(id, name, msg)` helper
  pair so each arm returns `reply(id, json!(...))` instead of rebuilding the
  envelope.
- Take `&Value` instead of `Value` in the three flagged signatures.
- Replace the repeated `"/".to_string()` cluster with a `const ROOT_PATH` or a
  small builder.

**Acceptance criteria.**
- `cargo clippy --all-targets -- -W clippy::pedantic` reports zero
  `needless_pass_by_value` warnings.
- Integration tests (`cargo test --test services`, `--test cli`, etc.) pass
  unchanged — the fake bridge's wire output is byte-identical.
- No `"/".to_string()` literal appears more than twice.

---

## #5 — Inconsistent error-hints contract across capabilities (~250 lines affected)

**Problem.** Only `firewall.rs` defines an `error_hints` function and threads it
through `render_with_hints`. `packages`, `packages_pk`, `services`, and
`network` route errors without capability-specific remediation hints, so a
`DependencyMissing` from `network` gives the user no follow-up command while the
same class of error from `firewall` does. The hint contract exists
(`render_with_hints(cli, view, error_hints)`) but is applied in one of five
capabilities — an inconsistent, surprise-prone API surface spanning all five
dispatch paths (~50 lines of dispatch/render glue each).

**Fix.**
- Decide the contract deliberately: either (a) every capability supplies an
  `error_hints` fn (even if it returns `None` for most variants), or (b) hints
  move to a single `FezError`-level method (`fn hints(&self) -> Option<Value>`)
  so the behavior is uniform and lives next to the error definition in
  `error.rs`. Option (b) is the lazier, more consistent fix and removes the
  per-capability glue.
- If (b): `render_with_hints` calls `err.hints()` directly; drop the
  `error_hints` parameter and the firewall-specific fn folds into the enum.

**Acceptance criteria.**
- Every capability produces consistent hint behavior for `DependencyMissing`
  and `UnsupportedApi` (test each capability's error path emits or omits hints
  by the same rule).
- The chosen contract is documented in `docs/agents/capabilities.md`.
- `make check` passes; error codes and exit codes (stable API) are unchanged.

---

## Summary ranking

| Rank | Smell | Lines affected | Primary fix |
|------|-------|---------------|-------------|
| 1 | God modules | ~3,172 | Split firewall.rs, fake_bridge.rs, capability/mod.rs |
| 2 | Over-long functions | ~1,094 | Extract per-branch helpers; data-drive schemas |
| 3 | Static metadata in match arms | ~336 | Move flag/output schemas to const tables |
| 4 | Fake-bridge reply boilerplate | ~290 | reply() helper; borrow `&Value` |
| 5 | Inconsistent error-hints contract | ~250 | Move hints to `FezError::hints()` |

Notes:
- Production `unwrap`/`expect` is well-controlled (6 unwrap, 9 expect; most in
  the fake-bridge test harness). Not a top-5 smell.
- Cross-file copy-paste duplication is low (one significant repeated block).
- #1 and #2 overlap by design: shrinking the god modules naturally splits the
  long functions, so tackle #1 first and #2 partly resolves.
