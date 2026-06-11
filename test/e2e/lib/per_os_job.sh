#!/usr/bin/env bash
# One OS job: provision -> wait -> run capabilities -> record -> teardown.
# Run in a subshell by run.sh so the EXIT trap is job-scoped.
# Caller globals: HERE, TF_SRC, FEZ_BIN, FEZ_VERSION, RESULT_FILE.
# Sourced libs: assertions.sh, capabilities.sh.
#
# Failures are never auto-filed as issues; they are recorded in RESULT_FILE
# (surfaced in the final matrix table) and the forensic detail is echoed
# inline before teardown wipes $WORK, so the per-OS log is the record. File
# tickets by hand from that output when a real failure warrants one.

# emit_forensics <header> <file>: echo a labeled forensic block to stdout
# (the per-OS log) before teardown removes the temp dir holding <file>.
emit_forensics() {
  echo "---- $1 ----"
  cat "$2" 2>/dev/null || true
  echo "----------------------------"
}

run_os_job() {
  OS="$1"
  local WORK; WORK="$(mktemp -d)"
  local TF="$WORK/tf"
  STEP_LOG="$WORK/steps.log"; : >"$STEP_LOG"
  local infra_log="$WORK/infra.log"

  # shellcheck disable=SC2329  # cleanup is invoked indirectly via `trap cleanup EXIT` below.
  cleanup() {
    # Powering the guest off before destroy lets EC2 release it faster than
    # tearing down a running instance; best-effort, never blocks teardown.
    if [[ -n "${FEZ_SSH_CONFIG:-}" ]]; then
      ssh -F "$FEZ_SSH_CONFIG" -o BatchMode=yes \
        -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 \
        target 'sudo systemctl poweroff' >/dev/null 2>&1 || true
      sleep 5
    fi
    terraform -chdir="$TF" destroy -auto-approve -var "os=$OS" >>"$infra_log" 2>&1 || true
    rm -rf "$WORK"
  }
  trap cleanup EXIT

  # Isolated state: copy terraform sources into this job's private dir.
  cp -r "$TF_SRC/." "$TF/"

  if ! terraform -chdir="$TF" init -input=false >>"$infra_log" 2>&1; then
    echo "$OS infra fail" >>"$RESULT_FILE"
    emit_forensics "$OS infra: terraform init failed" "$infra_log"; return 0
  fi
  if ! terraform -chdir="$TF" apply -auto-approve -input=false -var "os=$OS" >>"$infra_log" 2>&1; then
    echo "$OS infra fail" >>"$RESULT_FILE"
    emit_forensics "$OS infra: terraform apply failed" "$infra_log"; return 0
  fi

  local IP USER KEY
  IP="$(terraform -chdir="$TF" output -raw public_ip)"
  USER="$(terraform -chdir="$TF" output -raw ssh_user)"
  KEY="$TF/$(terraform -chdir="$TF" output -raw key_path)"

  # Hermetic SSH config pinned via FEZ_SSH_CONFIG (ssh -F), never HOME.
  mkdir -p "$WORK/.ssh"
  cp "$KEY" "$WORK/.ssh/id"; chmod 600 "$WORK/.ssh/id"
  local SSH_CONFIG="$WORK/.ssh/config"
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

  # Wait for cloud-init to install the capability surface (5-minute budget).
  # Cap every probe so one wedged ssh can't outlive the per-iteration sleep:
  # ConnectTimeout fails a never-completing connect fast, and ServerAlive*
  # tears down a connection that stalls *after* connecting (sshd accepted TCP
  # mid-cloud-init then hung). Both bound a single probe to <10s, the loop step.
  local ready=
  local _
  for _ in $(seq 1 30); do
    if ssh -F "$SSH_CONFIG" -o BatchMode=yes \
        -o ConnectTimeout=5 -o ServerAliveInterval=5 -o ServerAliveCountMax=1 \
        target 'test -f /var/lib/fez-e2e-ready' 2>/dev/null; then
      ready=1; break
    fi
    sleep 10
  done
  if [[ -z "$ready" ]]; then
    ssh -F "$SSH_CONFIG" -o BatchMode=yes \
      -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 \
      target \
      'cloud-init status --long 2>&1; sudo tail -n 40 /var/log/cloud-init-output.log 2>&1' \
      >>"$infra_log" 2>&1 || true
    echo "$OS infra fail" >>"$RESULT_FILE"
    emit_forensics "$OS infra: host never became ready (no /var/lib/fez-e2e-ready after 5m)" "$infra_log"
    return 0
  fi

  # Run each capability; record status; echo forensics inline on failure.
  # Capture the full per-step output to a temp so the PASS/FAIL banners land in
  # the job log (this stdout is tee'd by run.sh), then read the terminal status
  # line. Echoing the banners is what makes a failure debuggable after teardown.
  local cap status capout
  capout="$WORK/cap.out"
  for cap in services packages network firewall; do
    echo "== $OS: $cap =="
    : >"$STEP_LOG"
    "test_$cap" >"$capout" 2>&1 || true
    cat "$capout"
    status="$(tail -n1 "$capout")"
    echo "$OS $cap $status" >>"$RESULT_FILE"
    if [[ "$status" == "fail" ]]; then
      # Surface the forensic step log inline before teardown wipes it.
      emit_forensics "$OS $cap forensics" "$STEP_LOG"
    fi
  done
}
