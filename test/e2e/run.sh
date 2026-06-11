#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TF_SRC="$HERE/terraform"
FEZ_BIN="${FEZ_BIN:-$(cd "$HERE/../.." && pwd)/target/debug/fez}"
# Derived unconditionally so both the parent and the re-exec'd child (which
# skips the logging block below) can reference it under `set -u`.
LOG_DIR="${LOG_DIR:-$HERE/logs}"

# Self-re-exec through tee so the whole matrix run is captured to a log that
# survives host teardown. last-run.log always points at the newest matrix run.
if [[ -z "${FEZ_E2E_LOGGING:-}" ]]; then
  mkdir -p "$LOG_DIR"
  LOG_FILE="${LOG_FILE:-$LOG_DIR/matrix-$(date +%Y%m%d-%H%M%S).log}"
  export FEZ_E2E_LOGGING=1 LOG_FILE
  echo "Logging to $LOG_FILE"
  "$0" "$@" 2>&1 | tee "$LOG_FILE"
  status=${PIPESTATUS[0]}
  ln -sf "$(basename "$LOG_FILE")" "$LOG_DIR/last-run.log"
  echo "Full log: $LOG_FILE"
  exit "$status"
fi

# OS matrix: default fedora + rhel10, overridable via FEZ_E2E_OS (space list).
read -r -a OSES <<<"${FEZ_E2E_OS:-fedora rhel10}"

FEZ_VERSION="$(git -C "$HERE/../.." rev-parse --short HEAD 2>/dev/null || echo unknown)"
RESULT_FILE="$(mktemp)"
export FEZ_BIN FEZ_VERSION RESULT_FILE HERE TF_SRC

# Source the libs once; they are pure function definitions.
# shellcheck source=test/e2e/lib/assertions.sh
source "$HERE/lib/assertions.sh"
# shellcheck source=test/e2e/lib/capabilities.sh
source "$HERE/lib/capabilities.sh"
# shellcheck source=test/e2e/lib/issue.sh
source "$HERE/lib/issue.sh"
# shellcheck source=test/e2e/lib/per_os_job.sh
source "$HERE/lib/per_os_job.sh"

# Fan out: one backgrounded subshell per OS, each tee'd to its own log.
# The subshell isolates run_os_job's EXIT trap to that job.
pids=()
for os in "${OSES[@]}"; do
  os_log_dir="$LOG_DIR/$os"
  mkdir -p "$os_log_dir"
  os_log="$os_log_dir/run-$(date +%Y%m%d-%H%M%S).log"
  (
    run_os_job "$os" 2>&1 | tee "$os_log"
    ln -sf "$(basename "$os_log")" "$os_log_dir/last-run.log"
  ) &
  pids+=("$!")
done

# Wait for every job; a job never aborts the matrix (failures are recorded).
for pid in "${pids[@]}"; do
  wait "$pid" || true
done

# Aggregate + print the matrix table.
echo
echo "==== E2E MATRIX RESULTS ===="
printf '%-10s %-12s %s\n' OS CAPABILITY STATUS
fail_count=0
while read -r os cap status; do
  [[ -z "$os" ]] && continue
  printf '%-10s %-12s %s\n' "$os" "$cap" "$status"
  [[ "$status" == "fail" ]] && fail_count=$((fail_count+1))
done <"$RESULT_FILE"
echo "============================"
rm -f "$RESULT_FILE"

if [[ "$fail_count" -gt 0 ]]; then
  echo "E2E MATRIX FAILED ($fail_count failing cells; issues filed)"
  exit 1
fi
echo "E2E MATRIX PASSED"
