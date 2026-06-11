#!/usr/bin/env bash
# Provisioning phase for the phased e2e matrix.
#
# provision_all <os...> does ONE terraform apply over all requested OSes
# (aws_instance.e2e for_each = var.oses, Terraform parallelizes them) and lets
# Terraform own the readiness wait via a per-host remote-exec provisioner with
# a hard timeout. A host that never readies taints its instance and makes the
# apply non-zero, but siblings still come up; we tolerate the non-zero exit and
# derive survivors from `terraform output -json`. Survivors land in the HOST_*
# registry the test phases consume; requested-but-absent OSes are recorded as
# infra fails and dropped.
#
# Caller globals: TF_SRC, RESULT_FILE.
# Populated registry (associative arrays, keyed by OS):
#   HOST_SSH[os]   ssh -F config path
#   HOST_WORK[os]  per-host scratch dir (ssh config, audit sinks, step logs)
# Shared globals:
#   TF_DIR         the single terraform working dir (kept until teardown)
#   TF_INFRA_LOG   provision/teardown forensic log
#   READY_OSES     ordered list of OSes that came up

# HOST_SSH/HOST_WORK are consumed by run.sh's test phase across the source
# boundary; shellcheck cannot see that use from this file.
# shellcheck disable=SC2034
declare -A HOST_SSH HOST_WORK
READY_OSES=()
TF_DIR=""
TF_INFRA_LOG=""

# provision_all <os...>: single apply, then populate READY_OSES from outputs.
provision_all() {
  local oses=("$@")
  TF_DIR="$(mktemp -d)/tf"
  TF_INFRA_LOG="$(dirname "$TF_DIR")/infra.log"
  : >"$TF_INFRA_LOG"
  mkdir -p "$TF_DIR"
  cp -r "$TF_SRC/." "$TF_DIR/"

  # Render the OS set as an HCL list literal: ["fedora","rhel10"].
  local hcl_list elem first=1
  hcl_list="["
  for elem in "${oses[@]}"; do
    [[ $first -eq 1 ]] || hcl_list+=","
    hcl_list+="\"$elem\""
    first=0
  done
  hcl_list+="]"

  echo "provisioning: ${oses[*]} (single apply, terraform-parallel)"
  if ! terraform -chdir="$TF_DIR" init -input=false >>"$TF_INFRA_LOG" 2>&1; then
    echo "FATAL: terraform init failed" >>"$TF_INFRA_LOG"
    _record_all_infra_fail "${oses[@]}"
    return 0
  fi
  # Tolerate a non-zero apply: a host that fails the readiness provisioner
  # taints only its own instance; survivors still come up and appear in the
  # output maps. We never abort here on the apply's exit status.
  terraform -chdir="$TF_DIR" apply -auto-approve -input=false \
    -var "oses=$hcl_list" >>"$TF_INFRA_LOG" 2>&1 || true

  # Survivors = OSes present in the output map (their instance came up AND
  # passed the readiness provisioner). Parse once.
  local out; out="$(terraform -chdir="$TF_DIR" output -json 2>>"$TF_INFRA_LOG")"
  local key; key="$TF_DIR/$(jq -r '.key_path.value' <<<"$out")"
  local up_oses
  up_oses="$(jq -r '.public_ips.value | keys[]' <<<"$out" 2>/dev/null)"

  local os
  for os in "${oses[@]}"; do
    if ! grep -qx "$os" <<<"$up_oses"; then
      echo "$os: INFRA FAIL (no host in terraform output; see $TF_INFRA_LOG)"
      echo "$os infra fail" >>"$RESULT_FILE"
      continue
    fi
    local ip user work ssh_config
    ip="$(jq -r --arg o "$os" '.public_ips.value[$o]' <<<"$out")"
    user="$(jq -r --arg o "$os" '.ssh_users.value[$o]' <<<"$out")"
    work="$(mktemp -d)"
    ssh_config="$work/.ssh/config"
    mkdir -p "$work/.ssh"
    cp "$key" "$work/.ssh/id"; chmod 600 "$work/.ssh/id"
    # Hermetic SSH config pinned via ssh -F, never HOME. Connection caps keep a
    # half-open or stalled connection from wedging any later one-shot ssh in
    # the capability tests (issue #49): ConnectTimeout fails a connect that
    # never completes; ServerAlive* tears down a connection that stalls after
    # connecting.
    cat >"$ssh_config" <<EOF
Host target
  HostName $ip
  User $user
  IdentityFile $work/.ssh/id
  IdentitiesOnly yes
  UserKnownHostsFile $work/.ssh/known_hosts
  StrictHostKeyChecking accept-new
  ConnectTimeout 10
  ServerAliveInterval 5
  ServerAliveCountMax 2
EOF
    HOST_WORK[$os]="$work"
    HOST_SSH[$os]="$ssh_config"
    READY_OSES+=("$os")
    echo "$os: ready ($ip)"
  done

  if [[ ${#READY_OSES[@]} -eq 0 ]]; then
    echo "---- provision forensics (no hosts came up) ----"
    cat "$TF_INFRA_LOG" 2>/dev/null || true
    echo "------------------------------------------------"
  fi
}

# _record_all_infra_fail <os...>: mark every OS infra-fail (init/fatal path).
_record_all_infra_fail() {
  local os
  for os in "$@"; do
    echo "$os: INFRA FAIL"
    echo "$os infra fail" >>"$RESULT_FILE"
  done
  echo "---- provision forensics ----"
  cat "$TF_INFRA_LOG" 2>/dev/null || true
  echo "-----------------------------"
}
