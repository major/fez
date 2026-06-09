#!/usr/bin/env python3
"""Enforce docstring (doc comment) coverage from rustdoc --show-coverage JSON.

Reads the JSON emitted by:

    cargo +nightly rustdoc --lib -- \
        -Z unstable-options --show-coverage --output-format=json

The JSON is a flat map of file path -> {"total": N, "with_docs": M, ...}.
This sums with_docs/total across all files, prints the percentage, and exits
non-zero when it is below the DOCS_MIN environment variable (default 100).
"""

from __future__ import annotations

import json
import os
import sys


def main() -> int:
    min_pct = float(os.environ.get("DOCS_MIN", "100"))

    try:
        data = json.load(sys.stdin)
    except json.JSONDecodeError as exc:
        print(f"docs-coverage: could not parse rustdoc JSON: {exc}", file=sys.stderr)
        return 2

    total = 0
    documented = 0
    for stats in data.values():
        total += stats.get("total", 0)
        documented += stats.get("with_docs", 0)

    if total == 0:
        print("docs-coverage: no documentable items found", file=sys.stderr)
        return 2

    pct = 100.0 * documented / total
    print(f"docstring coverage: {documented}/{total} ({pct:.1f}%), min {min_pct:.0f}%")

    if pct + 1e-9 < min_pct:
        print(
            f"docs-coverage: FAILED ({pct:.1f}% < {min_pct:.0f}%)",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
