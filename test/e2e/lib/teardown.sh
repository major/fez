#!/usr/bin/env bash
# Teardown phase for the phased e2e matrix.
#
# teardown_all destroys the single shared terraform stack (every provisioned
# host, including any tainted/never-ready instances) and is wired to run.sh's
# EXIT trap, so hosts are torn down on success, failure, or Ctrl-C. Cost
# control: no instance survives a crashed run.
#
# Caller globals: TF_DIR, TF_INFRA_LOG.

teardown_all() {
  [[ -n "${TF_DIR:-}" && -d "$TF_DIR" ]] || return 0
  echo "tearing down all hosts ..."
  # Best-effort: never let a destroy failure mask the run's real exit status.
  terraform -chdir="$TF_DIR" destroy -auto-approve -input=false \
    >>"${TF_INFRA_LOG:-/dev/null}" 2>&1 || \
    echo "WARNING: terraform destroy reported errors; check $TF_INFRA_LOG and AWS for orphans"
}
