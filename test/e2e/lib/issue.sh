#!/usr/bin/env bash
# GitHub issue dedupe/create/comment + redaction for the fez e2e matrix.
# Caller globals: OS, FEZ_VERSION, AMI_NAME. Requires an authenticated `gh`.

# redact: strip host/account secrets from stdin so they never land in an issue.
#   - IPv4 addresses
#   - absolute paths under /tmp (ephemeral ssh config + key)
#   - AWS account ids (12-digit) and ARNs
redact() {
  sed -E \
    -e 's/[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}/<redacted-ip>/g' \
    -e 's#/tmp/[A-Za-z0-9._/-]+#<redacted-tmp-path>#g' \
    -e 's/arn:aws[A-Za-z0-9:._/-]+/<redacted-arn>/g' \
    -e 's/([^0-9]|^)[0-9]{12}([^0-9]|$)/\1<redacted-acct>\2/g'
}

# _marker <os> <capability> -> the hidden HTML comment used for dedupe.
_marker() { echo "<!-- fez-e2e:$1:$2 -->"; }

# _find_open <marker> -> issue number of the first open issue carrying marker, or empty.
_find_open() {
  gh issue list --state open --search "$1 in:body" \
    --json number,body \
    --jq "map(select(.body | contains(\"$1\"))) | (.[0].number // empty)"
}

# _ensure_label: create the `e2e` label if the repo lacks it. Idempotent and
# best-effort; a missing label must not abort issue creation (gh issue create
# hard-fails on an unknown --label, so guarantee it exists first).
_ensure_label() {
  gh label create e2e \
    --description "fez end-to-end matrix failures" \
    --color B60205 >/dev/null 2>&1 || true
}

# _file <title> <marker> <body-file>
# Comment on the existing open issue if the marker matches, else create one.
_file() {
  local title="$1" marker="$2" body_file="$3" num
  num="$(_find_open "$marker")"
  local ts; ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if [[ -n "$num" ]]; then
    { echo "Recurred on e2e run at $ts."; echo; cat "$body_file"; } \
      | gh issue comment "$num" --body-file -
    echo "commented on #$num: $title"
  else
    _ensure_label
    gh issue create --title "$title" --label e2e --body-file "$body_file" >/dev/null
    echo "created issue: $title"
  fi
}

# file_capability_issue <capability> <step-log-file>
# One issue per (OS, capability) capability failure.
file_capability_issue() {
  local cap="$1" steplog="$2" marker; marker="$(_marker "$OS" "$cap")"
  local body; body="$(mktemp)"
  {
    echo "$marker"
    echo "## e2e failure: $OS / $cap"
    echo
    echo "- OS: \`$OS\`"
    echo "- AMI: \`${AMI_NAME:-unknown}\`"
    echo "- fez: \`${FEZ_VERSION:-unknown}\`"
    echo
    echo "### Failed assertions"
    echo
    echo '```text'
    redact <"$steplog"
    echo '```'
    echo
    echo "### Reproduce locally"
    echo
    echo '```bash'
    echo "FEZ_E2E_OS=$OS test/e2e/run.sh"
    echo '```'
  } >"$body"
  _file "e2e: $OS $cap failing" "$marker" "$body"
  rm -f "$body"
}

# file_infra_issue <reason> <detail-file>
# Single per-OS infra issue (apply error, host never ready, install failure).
file_infra_issue() {
  local reason="$1" detail="$2" marker; marker="$(_marker "$OS" "infra")"
  local body; body="$(mktemp)"
  {
    echo "$marker"
    echo "## e2e infra failure: $OS"
    echo
    echo "- OS: \`$OS\`"
    echo "- AMI: \`${AMI_NAME:-unknown}\`"
    echo "- fez: \`${FEZ_VERSION:-unknown}\`"
    echo "- reason: $reason"
    echo
    echo "### Detail"
    echo
    echo '```text'
    redact <"$detail"
    echo '```'
  } >"$body"
  _file "e2e: $OS infra failing" "$marker" "$body"
  rm -f "$body"
}
