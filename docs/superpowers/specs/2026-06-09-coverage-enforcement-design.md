# Coverage enforcement design

## Goal

Enforce three hard quality bars on `fez`, locally via a Makefile and in CI via GitHub Actions:

- 90% project test coverage
- 95% patch (diff) test coverage
- 100% docstring (doc comment) coverage

Test coverage flows through Codecov. Docstring coverage is enforced by the Makefile and a CI job.

## Context

`fez` is a Rust crate (library + thin binaries). Existing CI is `cd.yml` (release binaries) and `release-plz.yml`. There is no `ci.yml`, no Makefile, and no coverage tooling yet. Actions are SHA-pinned with `persist-credentials: false`.

Current docstring coverage is 25.1% (53 of 211 items). The 100% docstring gate will therefore fail in CI until a follow-up writes the missing doc comments. This is accepted: the infrastructure lands now, the docstrings come later, and CI stays red on the docs job until then.

## Tooling decisions

- Test coverage: `cargo-llvm-cov` (source-based LLVM instrumentation), installed in CI via `taiki-e/install-action`. Emits `lcov.info` for Codecov.
- Docstring coverage: nightly `cargo rustdoc --lib -- -Z unstable-options --show-coverage --output-format=json`, parsed by a small script that sums `with_docs` / `total` across files. The `rust-toolchain.toml` pins stable 1.92, so the docs target uses an explicit `cargo +nightly` override.
- Gate ownership: Codecov enforces project (>=90%) and patch (>=95%). The Makefile and CI enforce docstrings (100%).

## Components

### 1. Makefile

Targets:

- `test` - `cargo test`
- `coverage` - `cargo llvm-cov --lcov --output-path lcov.info` plus a printed summary
- `coverage-html` - local HTML report under `target/llvm-cov/html`
- `docs-coverage` - nightly rustdoc JSON, sum docs ratio, fail if below `DOCS_MIN`
- `lint` - `cargo clippy -- -D warnings` and `cargo fmt --check`
- `check` - runs `lint test docs-coverage` (one-shot local gate)
- `clean-coverage` - removes coverage artifacts

Tunables:

- `DOCS_MIN := 100` - docstring threshold percentage. Lower this only if intentionally relaxing the gate.

The `docs-coverage` parser is self-contained (python3, already present on dev and CI runners). It reads the per-file JSON objects, each shaped `{ "total": N, "with_docs": M, ... }`, sums `M` and `N` across all files, computes the percentage, prints it, and exits non-zero when below `DOCS_MIN`.

### 2. codecov.yml

- `coverage.status.project.default.target: 90%`
- `coverage.status.patch.default.target: 95%`
- `ignore:` test-only code that should not count toward project coverage:
  - `src/bin/fake_bridge.rs`
  - `tests/`

Both statuses are blocking PR checks (not informational).

### 3. .github/workflows/ci.yml

New workflow. Triggers: `push` to `main`, `pull_request`. Style matches existing workflows (SHA-pinned actions, `persist-credentials: false`, `if: github.repository == 'major-hayden/fez'` where appropriate).

Jobs:

- `lint` (stable): `cargo fmt --check`, `cargo clippy -D warnings`
- `coverage` (stable): install `cargo-llvm-cov`, run `make coverage`, upload `lcov.info` to Codecov with `secrets.CODECOV_TOKEN`
- `docs-coverage` (nightly): install nightly toolchain, run `make docs-coverage`. Blocking. Expected to fail until docstrings are written.

## Data flow

```text
PR opened
  ├─ ci.yml: lint          -> clippy + fmt gate
  ├─ ci.yml: docs-coverage -> nightly rustdoc -> 100% gate (Makefile)   [red until follow-up]
  └─ ci.yml: coverage      -> cargo-llvm-cov -> lcov.info -> Codecov
                                                   └─ project >= 90% (PR check)
                                                      patch   >= 95% (PR check)
```

## Out of scope

- Writing the 158 missing doc comments (separate follow-up).
- Windows/macOS coverage runs (Linux-only for now, matching the project's target audience).
