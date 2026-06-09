#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TF="$HERE/terraform"
WORK="$(mktemp -d)"
FEZ_BIN="${FEZ_BIN:-$(cd "$HERE/../.." && pwd)/target/debug/fez}"

# Capture the full run to a log so failures stay inspectable after the box is
# destroyed. Re-exec once, piping stdout+stderr through tee. LOG_DIR/LOG_FILE
# are overridable; last-run.log always points at the newest run.
if [[ -z "${FEZ_E2E_LOGGING:-}" ]]; then
  LOG_DIR="${LOG_DIR:-$HERE/logs}"
  mkdir -p "$LOG_DIR"
  LOG_FILE="${LOG_FILE:-$LOG_DIR/run-$(date +%Y%m%d-%H%M%S).log}"
  export FEZ_E2E_LOGGING=1 LOG_FILE
  echo "Logging to $LOG_FILE"
  set -o pipefail
  "$0" "$@" 2>&1 | tee "$LOG_FILE"
  status=${PIPESTATUS[0]}
  ln -sf "$(basename "$LOG_FILE")" "$LOG_DIR/last-run.log"
  echo "Full log: $LOG_FILE"
  exit "$status"
fi

cleanup() {
  terraform -chdir="$TF" destroy -auto-approve || true
  rm -rf "$WORK"
}
trap cleanup EXIT

terraform -chdir="$TF" init -input=false
terraform -chdir="$TF" apply -auto-approve -input=false

IP="$(terraform -chdir="$TF" output -raw public_ip)"
USER="$(terraform -chdir="$TF" output -raw ssh_user)"
KEY="$TF/$(terraform -chdir="$TF" output -raw key_path)"

# Hermetic SSH config so fez's `ssh` uses our ephemeral key + alias only.
# We pin it via FEZ_SSH_CONFIG (`ssh -F`) rather than overriding HOME: OpenSSH
# does not reliably read $HOME/.ssh/config for non-interactive invocations, so
# HOME=... silently fell back to the ambient config and every call failed.
mkdir -p "$WORK/.ssh"
cp "$KEY" "$WORK/.ssh/id"
chmod 600 "$WORK/.ssh/id"
SSH_CONFIG="$WORK/.ssh/config"
cat >"$SSH_CONFIG" <<EOF
Host target
  HostName $IP
  User $USER
  IdentityFile $WORK/.ssh/id
  IdentitiesOnly yes
  UserKnownHostsFile $WORK/.ssh/known_hosts
  StrictHostKeyChecking accept-new
EOF
export FEZ_SSH_CONFIG="$SSH_CONFIG"

# Wait for boot + cloud-init (cockpit-bridge install). Fail loudly if the box
# never signals readiness instead of marching into the first fez call blind.
ready=
for _ in $(seq 1 30); do
  if ssh -F "$SSH_CONFIG" -o BatchMode=yes target 'test -f /var/lib/fez-e2e-ready' 2>/dev/null; then
    ready=1
    break
  fi
  sleep 10
done
if [[ -z "$ready" ]]; then
  echo "FATAL: target never became ready (no /var/lib/fez-e2e-ready after 5m)" >&2
  echo "--- cloud-init status ---" >&2
  ssh -F "$SSH_CONFIG" -o BatchMode=yes -o ConnectTimeout=10 target \
    'cloud-init status --long 2>&1; sudo tail -n 40 /var/log/cloud-init-output.log 2>&1' >&2 || true
  exit 1
fi

echo "== services list =="
# First real bridge call: surface the payload on failure so a broken transport
# or unready bridge is diagnosable from the log, not a bare `jq` exit.
OUT="$("$FEZ_BIN" --host target services list --json)"
if ! echo "$OUT" | jq -e '.apiVersion == "fez/v1" and .status == "ok"' >/dev/null; then
  echo "FATAL: first fez call did not return ok; payload was:" >&2
  echo "$OUT" >&2
  exit 1
fi
echo "$OUT" | jq -e '.data.units | map(.name) | index("sshd.service")' >/dev/null

echo "== services status sshd =="
"$FEZ_BIN" --host target services status sshd.service --json |
  jq -e '.kind == "ServiceStatus" and .data.active_state == "active"' >/dev/null

echo "== services logs sshd =="
"$FEZ_BIN" --host target services logs sshd.service --lines 10 --json |
  jq -e '.kind == "LogEntries"' >/dev/null

# Audit lands on the runner (where fez executes and decides). Use the file sink
# for a deterministic assertion that does not depend on the runner's journal.
AUDIT="$WORK/audit.jsonl"
export FEZ_AUDIT="file:$AUDIT"

SAFE_UNIT="chronyd.service"

echo "== dry-run leaves state unchanged =="
"$FEZ_BIN" --host target services stop "$SAFE_UNIT" --dry-run --json |
  jq -e '.kind == "DryRun" and .data.operation == "stop"' >/dev/null
"$FEZ_BIN" --host target services status "$SAFE_UNIT" --json |
  jq -e '.data.active_state == "active"' >/dev/null

echo "== protected unit refused without --force =="
set +e
"$FEZ_BIN" --host target services stop sshd.service --json >"$WORK/refusal.json" 2>/dev/null
rc=$?
set -e
[ "$rc" -eq 8 ] || { echo "expected exit 8, got $rc"; exit 1; }
jq -e '.error.code == "protected-unit"' <"$WORK/refusal.json" >/dev/null
"$FEZ_BIN" --host target services status sshd.service --json |
  jq -e '.data.active_state == "active"' >/dev/null

echo "== stop / start / restart a safe unit =="
"$FEZ_BIN" --host target services stop "$SAFE_UNIT" --json |
  jq -e '.kind == "ServiceMutation"' >/dev/null
"$FEZ_BIN" --host target services status "$SAFE_UNIT" --json |
  jq -e '.data.active_state == "inactive"' >/dev/null
"$FEZ_BIN" --host target services start "$SAFE_UNIT" --json |
  jq -e '.hints.reverse == "fez services stop chronyd.service"' >/dev/null
"$FEZ_BIN" --host target services status "$SAFE_UNIT" --json |
  jq -e '.data.active_state == "active"' >/dev/null
"$FEZ_BIN" --host target services restart "$SAFE_UNIT" --json |
  jq -e '.kind == "ServiceMutation"' >/dev/null

echo "== disable / enable a safe unit =="
"$FEZ_BIN" --host target services disable "$SAFE_UNIT" --json |
  jq -e '.kind == "ServiceEnablement"' >/dev/null
"$FEZ_BIN" --host target services status "$SAFE_UNIT" --json |
  jq -e '.data.unit_file_state == "disabled"' >/dev/null
"$FEZ_BIN" --host target services enable "$SAFE_UNIT" --json |
  jq -e '.hints.reverse == "fez services disable chronyd.service"' >/dev/null
"$FEZ_BIN" --host target services status "$SAFE_UNIT" --json |
  jq -e '.data.unit_file_state == "enabled"' >/dev/null

echo "== audit recorded attempt + result records =="
# Each executed mutation writes two records (attempt + result). The audited
# sequence above is stop, start, restart, disable, enable = 5 mutations, so 10
# records. Assert at least that many, plus at least one attempt and one ok.
grep -q '"result":"attempt"' "$AUDIT"
grep -q '"result":"ok"' "$AUDIT"
test "$(grep -c '"result":' "$AUDIT")" -ge 10   # 5 mutations * 2 records

echo "== audit journal smoke check (best-effort, non-fatal) =="
# Exercise the default JournalSink against the runner's journal if available.
if command -v journalctl >/dev/null 2>&1; then
  FEZ_AUDIT="" "$FEZ_BIN" --host target services restart "$SAFE_UNIT" >/dev/null 2>&1 || true
  journalctl --identifier=fez -n 5 --no-pager >/dev/null 2>&1 \
    && echo "journal readable" || echo "journal not readable on runner (skipped)"
else
  echo "journalctl unavailable on runner (skipped)"
fi

echo "E2E PASSED"
