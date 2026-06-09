#!/usr/bin/env bash
# Regenerate the vendored-crates tarball consumed as Source1 by fez.spec.
# Run from anywhere: packaging/make-vendor.sh <version>
#
# The companion source tarball (Source0) is the crate subtree with the crate at
# its root, produced from the repository's `fez/` directory, e.g.:
#   git archive --prefix=fez-<version>/ -o fez-<version>.tar.gz HEAD:fez
# Packit builds the same archive via its `create-archive` action (.packit.yaml).
set -euo pipefail
VERSION="${1:?usage: make-vendor.sh <version>}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"   # crate dir (contains Cargo.toml)
OUT="$HERE/packaging"

cd "$HERE"
rm -rf vendor
# Pin the exact dependency set the lockfile resolves; offline-buildable in mock.
cargo vendor --locked vendor >"$OUT/cargo-vendor-config.toml"
tar -caf "$OUT/fez-${VERSION}-vendor.tar.xz" vendor
rm -rf vendor
echo "wrote $OUT/fez-${VERSION}-vendor.tar.xz"
echo "cargo-vendor-config.toml holds the [source] replacement stanza (informational;"
echo "the spec's %cargo_prep -v vendor regenerates .cargo/config.toml at build time)."
