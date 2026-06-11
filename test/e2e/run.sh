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
# timeout before failing with a cryptic "no EC2 IMDS role found". One cheap STS
# call up front turns that into an immediate, actionable error.
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
# shellcheck source=test/e2e/lib/provision.sh
source "$HERE/lib/provision.sh"
# shellcheck source=test/e2e/lib/teardown.sh
source "$HERE/lib/teardown.sh"

# Phased model:
#   1. provision_all  - one terraform apply over all OSes (terraform-parallel),
#                       terraform owns the bounded readiness wait. Survivors
#                       land in HOST_SSH/HOST_WORK; dead hosts are infra-fail.
#   2. test phase     - for each capability, run it against EVERY ready host.
#   3. teardown_all   - EXIT trap destroys the single shared stack no matter
#                       how we leave (success, failure, Ctrl-C).
trap teardown_all EXIT

provision_all "${OSES[@]}"

# Capability list. Each maps to a test_<cap> function in capabilities.sh.
CAPS=(services packages network firewall)

# For each ready host, run every capability and record os/cap/status. Per-host
# context (OS, FEZ_SSH_CONFIG, WORK, STEP_LOG) is set before each call so the
# capability fns and the ssh transport target the right host. Forensics for a
# failing cell are echoed inline (the matrix log is the record; no auto-filed
# issues).
for os in "${READY_OSES[@]}"; do
  export OS="$os"
  export FEZ_SSH_CONFIG="${HOST_SSH[$os]}"
  WORK="${HOST_WORK[$os]}"
  for cap in "${CAPS[@]}"; do
    STEP_LOG="$WORK/steps-$cap.log"; : >"$STEP_LOG"
    echo "==== $os / $cap ===="
    status="$(test_"$cap")"
    echo "$os $cap $status" >>"$RESULT_FILE"
    if [[ "$status" == "fail" ]]; then
      echo "---- $os $cap forensics ----"
      cat "$STEP_LOG" 2>/dev/null || true
      echo "----------------------------"
    fi
  done
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
  echo "E2E MATRIX FAILED ($fail_count failing cells; see forensics above)"
  exit 1
fi
echo "E2E MATRIX PASSED"
