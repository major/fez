#!/usr/bin/env bash
# One OS job: provision -> wait -> run capabilities -> record -> teardown.
# Run in a subshell by run.sh so the EXIT trap is job-scoped.
# Caller globals: HERE, TF_SRC, FEZ_BIN, FEZ_VERSION, RESULT_FILE.
# Sourced libs: assertions.sh, capabilities.sh, issue.sh.

run_os_job() {
  OS="$1"
  local WORK; WORK="$(mktemp -d)"
  local TF="$WORK/tf"
  STEP_LOG="$WORK/steps.log"; : >"$STEP_LOG"
  local infra_log="$WORK/infra.log"
  AMI_NAME="unknown"

  # shellcheck disable=SC2329  # cleanup is invoked indirectly via `trap cleanup EXIT` below.
  cleanup() {
    terraform -chdir="$TF" destroy -auto-approve -var "os=$OS" >>"$infra_log" 2>&1 || true
    rm -rf "$WORK"
  }
  trap cleanup EXIT

  # Isolated state: copy terraform sources into this job's private dir.
  cp -r "$TF_SRC/." "$TF/"

  if ! terraform -chdir="$TF" init -input=false >>"$infra_log" 2>&1; then
    echo "$OS infra fail" >>"$RESULT_FILE"
    file_infra_issue "terraform init failed" "$infra_log"; return 0
  fi
  if ! terraform -chdir="$TF" apply -auto-approve -input=false -var "os=$OS" >>"$infra_log" 2>&1; then
    echo "$OS infra fail" >>"$RESULT_FILE"
    file_infra_issue "terraform apply failed" "$infra_log"; return 0
  fi

  local IP USER KEY
  IP="$(terraform -chdir="$TF" output -raw public_ip)"
  USER="$(terraform -chdir="$TF" output -raw ssh_user)"
  KEY="$TF/$(terraform -chdir="$TF" output -raw key_path)"
  # shellcheck disable=SC2034  # AMI_NAME is consumed by issue.sh (file_*_issue), a sibling sourced lib.
  AMI_NAME="$(terraform -chdir="$TF" output -raw ami_name)"

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
  local ready=
  local _
  for _ in $(seq 1 30); do
    if ssh -F "$SSH_CONFIG" -o BatchMode=yes target 'test -f /var/lib/fez-e2e-ready' 2>/dev/null; then
      ready=1; break
    fi
    sleep 10
  done
  if [[ -z "$ready" ]]; then
    ssh -F "$SSH_CONFIG" -o BatchMode=yes -o ConnectTimeout=10 target \
      'cloud-init status --long 2>&1; sudo tail -n 40 /var/log/cloud-init-output.log 2>&1' \
      >>"$infra_log" 2>&1 || true
    echo "$OS infra fail" >>"$RESULT_FILE"
    file_infra_issue "host never became ready (no /var/lib/fez-e2e-ready after 5m)" "$infra_log"
    return 0
  fi

  # Run each capability; record status; file an issue per capability failure.
  local cap status
  for cap in services packages network firewall; do
    echo "== $OS: $cap =="
    : >"$STEP_LOG"
    status="$("test_$cap" | tail -n1)"
    echo "$OS $cap $status" >>"$RESULT_FILE"
    if [[ "$status" == "fail" ]]; then
      file_capability_issue "$cap" "$STEP_LOG"
    fi
  done
}
