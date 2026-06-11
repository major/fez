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

# Preflight: prove AWS credentials resolve before provisioning. Without this,
# terraform's AWS provider falls through to EC2 IMDS and burns a multi-minute
# timeout per OS before failing with a cryptic "no EC2 IMDS role found". One
# cheap STS call up front turns that into an immediate, actionable error.
if ! aws sts get-caller-identity >/dev/null 2>&1; then
  cat >&2 <<EOF
ERROR: no usable AWS credentials.

  terraform needs AWS credentials to provision the e2e hosts. None resolved
  (checked env vars, AWS_PROFILE, and ~/.aws). Set a working profile, e.g.:

    AWS_PROFILE=<profile> $0 $*

  Verify with: AWS_PROFILE=<profile> aws sts get-caller-identity
EOF
  exit 2
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
# shellcheck source=test/e2e/lib/per_os_job.sh
source "$HERE/lib/per_os_job.sh"

# Fan out: one backgrounded subshell per OS, each tee'd to its own log.
# The subshell isolates run_os_job's EXIT trap to that job.
#
# Logs are always written per-OS. FEZ_E2E_VERBOSE=1 also streams each job's
# output to the console live (prefixed with the OS so interleaved jobs stay
# legible); unset, the per-OS chatter stays in the file and only the matrix
# table at the end hits the console. Either way the paths are printed up
# front so you can `tail -f` them from another terminal.
verbose="${FEZ_E2E_VERBOSE:-}"
echo "Per-OS logs (tail -f to watch live):"
declare -A OS_LOG
for os in "${OSES[@]}"; do
  os_log_dir="$LOG_DIR/$os"
  mkdir -p "$os_log_dir"
  os_log="$os_log_dir/run-$(date +%Y%m%d-%H%M%S).log"
  OS_LOG[$os]="$os_log"
  echo "  $os: $os_log"
done

pids=()
for os in "${OSES[@]}"; do
  os_log_dir="$LOG_DIR/$os"
  os_log="${OS_LOG[$os]}"
  (
    # `|| true` so a failing run_os_job pipeline (set -e/pipefail is inherited)
    # never skips the symlink update; failures are recorded in RESULT_FILE.
    if [[ -n "$verbose" ]]; then
      # Stream to console (prefixed) and to the per-OS log at once.
      run_os_job "$os" 2>&1 | tee "$os_log" | sed -u "s/^/[$os] /" || true
    else
      run_os_job "$os" >"$os_log" 2>&1 || true
    fi
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
  echo "E2E MATRIX FAILED ($fail_count failing cells; see per-OS logs above)"
  exit 1
fi
echo "E2E MATRIX PASSED"
