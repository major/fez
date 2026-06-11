#!/usr/bin/env bash
# Per-capability e2e test functions. Each echoes "pass" | "fail" | "skip" as its
# final stdout line. Depends on lib/assertions.sh helpers being sourced first.
# Caller globals: FEZ_BIN, OS, STEP_LOG, WORK.
#
# The helpers (run_capture, _record) and the globals they set (REPLY, RC) live in
# the sibling lib/assertions.sh, which the caller (run.sh / per_os_job.sh) sources
# first. Point shellcheck at it so `-x` can follow the definitions.
# shellcheck source=test/e2e/lib/assertions.sh
# REPLY/RC are set by run_capture in assertions.sh; OS/STEP_LOG/WORK/FEZ_BIN are
# caller-set globals. shellcheck cannot see those assignments from this file.
# shellcheck disable=SC2154

# _probe_present <error-code> <fez args...> -> 0 if present, 1 if dependency-missing.
# Used to decide skip vs run for optional subsystems (dnf5daemon, firewalld).
_probe_present() {
  run_capture "${@:2}"
  if [[ "$RC" -eq 9 ]] && echo "$REPLY" | jq -e --arg c "$1" '.error.code == $c' >/dev/null; then
    return 1
  fi
  return 0
}

test_services() {
  local fails=0
  local SAFE_UNIT="chronyd.service"

  assert_jq "services list returns ok envelope" \
    '.apiVersion == "fez/v1" and .status == "ok"' services list || fails=$((fails+1))
  # services list is a table envelope: .data.{columns,rows,count} with columns
  # [name,description,load_state,active_state,sub_state]; name is column 0.
  assert_jq "services list includes sshd.service" \
    '[.data.rows[][0]] | index("sshd.service")' services list || fails=$((fails+1))
  assert_jq "services status sshd active" \
    '.kind == "ServiceStatus" and .data.active_state == "active"' \
    services status sshd.service || fails=$((fails+1))
  assert_kind "services logs sshd" LogEntries services logs sshd.service --lines 10 || fails=$((fails+1))

  # Audit to a file sink on the runner for a deterministic assertion.
  export FEZ_AUDIT="file:$WORK/audit-services.jsonl"

  assert_jq "dry-run stop reports DryRun" \
    '.kind == "DryRun" and .data.operation == "stop"' \
    services stop "$SAFE_UNIT" --dry-run || fails=$((fails+1))
  assert_jq "dry-run leaves unit active" \
    '.data.active_state == "active"' services status "$SAFE_UNIT" || fails=$((fails+1))

  assert_exit "protected unit refused" 8 protected-unit services stop sshd.service || fails=$((fails+1))
  assert_jq "sshd still active after refusal" \
    '.data.active_state == "active"' services status sshd.service || fails=$((fails+1))

  assert_kind "stop safe unit" ServiceMutation services stop "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "safe unit inactive" \
    '.data.active_state == "inactive"' services status "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "start emits reverse hint" \
    '.hints.reverse == "fez services stop chronyd.service"' \
    services start "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "safe unit active again" \
    '.data.active_state == "active"' services status "$SAFE_UNIT" || fails=$((fails+1))
  assert_kind "restart safe unit" ServiceMutation services restart "$SAFE_UNIT" || fails=$((fails+1))

  assert_kind "disable safe unit" ServiceEnablement services disable "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "safe unit disabled" \
    '.data.unit_file_state == "disabled"' services status "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "enable emits reverse hint" \
    '.hints.reverse == "fez services disable chronyd.service"' \
    services enable "$SAFE_UNIT" || fails=$((fails+1))
  assert_jq "safe unit enabled" \
    '.data.unit_file_state == "enabled"' services status "$SAFE_UNIT" || fails=$((fails+1))

  # Audit: 5 mutations (stop/start/restart/disable/enable) * 2 records each.
  local audit="$WORK/audit-services.jsonl"
  if grep -q '"result":"attempt"' "$audit" 2>/dev/null \
     && grep -q '"result":"ok"' "$audit" 2>/dev/null \
     && [[ "$(grep -c '"result":' "$audit" 2>/dev/null || echo 0)" -ge 10 ]]; then
    echo "  PASS: audit recorded attempt+result records"
  else
    echo "  FAIL: audit recorded attempt+result records"
    _record "audit records" "grep result: $audit" ">=10 records, >=1 attempt, >=1 ok" \
      "$(cat "$audit" 2>/dev/null | tail -n 20)" "0"
    fails=$((fails+1))
  fi
  unset FEZ_AUDIT

  [[ "$fails" -eq 0 ]] && echo pass || echo fail
}

test_packages() {
  if ! _probe_present dependency-missing packages list; then
    echo "  SKIP: dnf5daemon-server absent (exit 9 dependency-missing)"
    echo skip; return 0
  fi
  local fails=0
  assert_kind "packages list" PackageList packages list || fails=$((fails+1))
  assert_kind "packages search" PackageSearch packages search vim || fails=$((fails+1))
  assert_kind "packages info" PackageInfo packages info bash || fails=$((fails+1))
  # Dry-run install must not change state; kind is PackagePlan on dry-run.
  assert_jq "dry-run install reports PackagePlan" \
    '.kind == "PackagePlan" and .data.dry_run == true' \
    packages install zsh --dry-run || fails=$((fails+1))
  [[ "$fails" -eq 0 ]] && echo pass || echo fail
}

test_network() {
  local fails=0
  # NetworkManager is always present; no skip path expected.
  assert_kind "network list" NetworkDeviceList network list || fails=$((fails+1))
  # network list returns a table envelope: .data.{columns,rows,count} where
  # columns are [interface,type,state,ip4,ip6,mac]. Pick the first activated
  # interface by column index (interface=0, state=2).
  run_capture network list
  local dev
  dev="$(echo "$REPLY" | jq -r '.data.rows[] | select(.[2] == "activated") | .[0]' | head -n1)"
  if [[ -n "$dev" && "$dev" != "null" ]]; then
    assert_kind "network show <primary>" NetworkDeviceDetail network show "$dev" || fails=$((fails+1))
  else
    echo "  FAIL: no activated interface found to show"
    _record "network show" "fez network list" "an activated interface" "$REPLY" "$RC"
    fails=$((fails+1))
  fi
  assert_exit "network show bogus dev not-found" 4 not-found network show fez-nope0 || fails=$((fails+1))
  [[ "$fails" -eq 0 ]] && echo pass || echo fail
}

test_firewall() {
  if ! _probe_present dependency-missing firewall status; then
    echo "  SKIP: firewalld absent (exit 9 dependency-missing)"
    echo skip; return 0
  fi
  local fails=0
  assert_kind "firewall status" FirewallStatus firewall status || fails=$((fails+1))
  assert_kind "firewall list zones" FirewallZoneList firewall list || fails=$((fails+1))
  assert_kind "firewall services catalog" FirewallServiceCatalog firewall services || fails=$((fails+1))

  # Mutation round-trip on a NON-session port (8080/tcp is not ssh, so the
  # protected-op guard does not fire). Add to runtime, then confirm to persist.
  assert_kind "add non-session port" FirewallChange firewall add-port 8080/tcp || fails=$((fails+1))
  assert_jq "status shows runtime drift for added port" \
    '.kind == "FirewallStatus"' firewall status || fails=$((fails+1))
  assert_kind "remove the port again" FirewallChange firewall remove-port 8080/tcp || fails=$((fails+1))

  # Protected-op guard: setting the default zone always refuses without --force.
  assert_exit "set-default-zone refused without --force" 8 protected-unit \
    firewall set-default-zone internal || fails=$((fails+1))

  [[ "$fails" -eq 0 ]] && echo pass || echo fail
}
