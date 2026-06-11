#!/usr/bin/env bash
# Shared bash+jq assertion primitives for the fez e2e matrix.
# Caller globals: FEZ_BIN, OS, STEP_LOG (append-only forensic sink).
# Every helper prints a one-line PASS/FAIL banner and, on failure, appends a
# structured block to $STEP_LOG for issue bodies.

# _record <label> <command> <expected> <actual> <rc>
# Append a forensic block describing one failed assertion.
_record() {
  {
    echo "### FAILED: $1"
    echo "command : $2"
    echo "expected: $3"
    echo "exit    : $5"
    echo "actual  :"
    echo "$4" | sed 's/^/    /'
    echo
  } >>"$STEP_LOG"
}

# run_capture <fez args...> -> sets REPLY (stdout) and RC (exit code).
# Never aborts the shell; callers inspect RC.
run_capture() {
  set +e
  REPLY="$("$FEZ_BIN" --host target "$@" --json 2>/dev/null)"
  RC=$?
  set -e
}

# assert_kind <label> <expected-kind> <fez args...>
# Run fez, assert .status==ok and .kind==expected-kind.
assert_kind() {
  local label="$1" kind="$2"; shift 2
  run_capture "$@"
  if [[ "$RC" -eq 0 ]] && echo "$REPLY" | jq -e \
      --arg k "$kind" '.apiVersion == "fez/v1" and .status == "ok" and .kind == $k' >/dev/null; then
    echo "  PASS: $label"
    return 0
  fi
  echo "  FAIL: $label"
  _record "$label" "fez $* --json" "kind=$kind status=ok" "$REPLY" "$RC"
  return 1
}

# assert_jq <label> <jq-filter> <fez args...>
# Run fez and assert an arbitrary jq -e filter holds on the payload.
assert_jq() {
  local label="$1" filter="$2"; shift 2
  run_capture "$@"
  if [[ "$RC" -eq 0 ]] && echo "$REPLY" | jq -e "$filter" >/dev/null; then
    echo "  PASS: $label"
    return 0
  fi
  echo "  FAIL: $label"
  _record "$label" "fez $* --json" "jq: $filter" "$REPLY" "$RC"
  return 1
}

# assert_exit <label> <expected-rc> <error-code> <fez args...>
# Run fez expecting a NON-zero exit; assert RC and .error.code.
assert_exit() {
  local label="$1" want_rc="$2" code="$3"; shift 3
  run_capture "$@"
  if [[ "$RC" -eq "$want_rc" ]] && echo "$REPLY" | jq -e \
      --arg c "$code" '.error.code == $c' >/dev/null; then
    echo "  PASS: $label"
    return 0
  fi
  echo "  FAIL: $label"
  _record "$label" "fez $* --json" "exit=$want_rc error.code=$code" "$REPLY" "$RC"
  return 1
}
