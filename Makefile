# fez development tasks.
#
# Quality gates enforced here and in CI:
#   - 90% project test coverage  (Codecov)
#   - 95% patch test coverage    (Codecov)
#   - 100% docstring coverage    (docs-coverage target below)

# Minimum docstring coverage percentage. Lower only to intentionally relax the gate.
DOCS_MIN := 100

LCOV := lcov.info

.DEFAULT_GOAL := check

.PHONY: check test coverage coverage-html docs-coverage lint security deny machete msrv clean-coverage

# One-shot local gate: lint, tests, docstring coverage, and supply-chain checks.
check: lint test docs-coverage security

# Run the test suite.
test:
	cargo test

# Generate lcov coverage for Codecov and print a summary.
coverage:
	cargo llvm-cov --lcov --output-path $(LCOV)
	cargo llvm-cov report --summary-only

# Open-able HTML coverage report under target/llvm-cov/html.
coverage-html:
	cargo llvm-cov --html

# Enforce docstring (doc comment) coverage via nightly rustdoc.
# rust-toolchain.toml pins stable, so we override with +nightly explicitly.
docs-coverage:
	@cargo +nightly rustdoc --lib -- \
		-Z unstable-options --show-coverage --output-format=json 2>/dev/null \
		| DOCS_MIN=$(DOCS_MIN) python3 scripts/docs_coverage.py

# Formatting and clippy gate.
lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

# Supply-chain and dependency hygiene: advisories/licenses/bans/sources and
# unused-dependency detection.
security: deny machete

# RUSTSEC advisories, license policy, banned/duplicate crates, and source
# allow-listing. Config in deny.toml. Install: cargo install cargo-deny --locked
deny:
	cargo deny check

# Detect dependencies declared in Cargo.toml but never used.
# Install: cargo install cargo-machete --locked
machete:
	cargo machete

# Verify the crate actually compiles on the pinned MSRV, not just on stable.
# Requires the 1.92 toolchain: rustup toolchain install 1.92
msrv:
	cargo +1.92 check --all-targets

clean-coverage:
	cargo llvm-cov clean --workspace
	rm -f $(LCOV)

# Build fuzz targets to verify they compile and link. Require nightly.
# Install: cargo install cargo-fuzz --locked
fuzz-build:
	cd fuzz && cargo fuzz build
