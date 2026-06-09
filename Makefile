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

.PHONY: check test coverage coverage-html docs-coverage lint clean-coverage

# One-shot local gate: lint, tests, and docstring coverage.
check: lint test docs-coverage

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

clean-coverage:
	cargo llvm-cov clean --workspace
	rm -f $(LCOV)
