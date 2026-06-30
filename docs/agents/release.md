# Release And Gate Notes For Agents

Read this before changing CI, release, packaging, security policy, MSRV, supply-chain checks, or command gates.

## Local Commands

Use the Makefile, not raw cargo, for gated work. `make` defaults to `check`.

| Command | Purpose |
| --- | --- |
| `make check` | Local one-shot gate: `lint test docs-coverage security`. |
| `make lint` | `cargo fmt --check` then `cargo clippy --all-targets -- -D warnings`. |
| `make test` | `cargo test` for unit and integration tests. |
| `make coverage` / `make coverage-html` | lcov for Codecov or local HTML coverage. |
| `make docs-coverage` | Requires `cargo +nightly`; enforces 100% docstring coverage via `scripts/docs_coverage.py`. |
| `make security` | Runs `deny machete`. |
| `make deny` | `cargo deny check`; install with `cargo install cargo-deny --locked`. |
| `make machete` | `cargo machete`; install with `cargo install cargo-machete --locked`. |
| `make msrv` | `cargo +1.92 check --all-targets`; requires `rustup toolchain install 1.92`. |

Single integration test pattern: `cargo test --test services name_of_test`. Current integration files include `tests/cli.rs`, `tests/packages.rs`, `tests/packages_pk.rs`, `tests/services.rs`, `tests/network.rs`, and `tests/firewall.rs`.

## CI Gates

CI lives in `.github/workflows/ci.yml` and `.github/workflows/codeql.yml`. Do not regress these contracts:

- **90% project** and **95% patch** coverage through Codecov (`codecov.yml`).
- **100% docstring coverage**. Crate root denies missing docs; every public item needs a doc comment, and fallible methods need an `# Errors` section.
- `clippy -D warnings` and `cargo fmt --check`. Do not add `#![deny(warnings)]` to source; warning pressure lives in CI flags.
- **MSRV build**: `cargo +1.92 check --all-targets` must pass. The pinned MSRV is a contract.
- **Supply chain**: `cargo deny check` plus `cargo machete`.
- **Fuzz build**: `cargo +nightly fuzz build` in `ci.yml` verifies fuzz targets still compile and link. Not a fuzz run — just a bitrot guard. Requires `cargo-fuzz` installed locally. Reproducible via `make fuzz-build`.
- **CodeQL**: separate workflow, build-mode `none`, runs on push/PR to `main` and weekly. Rust CodeQL supports buildless extraction here; `manual` and `autobuild` are rejected at init. Not reproducible through `make check`.

CI actions are SHA-pinned. When bumping an action, pin the new commit SHA and keep the `# vX.Y.Z` comment accurate.

## Security And Dependency Hygiene

`deny.toml` controls advisories, license policy, banned/duplicate crates, and source allow-list. The license allow-list is intentionally tight: Apache-2.0, MIT, Unicode-3.0, and Unlicense. A dependency with an unlisted license fails CI on purpose.

`cargo machete` catches unused declared dependencies. If a dependency is intentionally retained for non-obvious reasons, document that reason close to the declaration or relevant code.

## Release Flow

`release-plz` handles version bumps and changelog through `.github/workflows/release-plz.yml`, `.github/workflows/cd.yml`, and `release-plz.toml`.

The crates.io package name is `rusty-fez` because `fez` is already taken. Release binaries and RPMs still install the `fez` command.

## Packaging

RPM packaging lives in `packaging/`:

- `packaging/fez.spec` - RPM spec.
- `packaging/make-vendor.sh` - vendoring helper.

Hidden `fez man` emits the `fez.1` roff page; packaging runs it during `%build`.
