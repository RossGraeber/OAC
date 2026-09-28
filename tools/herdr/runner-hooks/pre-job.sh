#!/usr/bin/env bash
# Pre-job hook for the operator-owned `oac-harness` runners (Epic K, K6 #129).
#
# GitHub runs this before any step of any job assigned to the runner when the runner's
# environment sets ACTIONS_RUNNER_HOOK_JOB_STARTED to its absolute path. Exit 0 lets the
# job run; any other exit fails the job before its first step. Default deny: only the
# opt-in workflow, from main, on a dispatch or a push, in this repository, gets through.
# A fork cannot change this file: it lives on the runner machine, not in the repository.
#
# Install it OUTSIDE the runner directory, owned by root and not writable by the runner
# user (e.g. /usr/local/lib/oac-harness/pre-job.sh, mode 0755), and set the variable in the
# runner service's own environment. Setup and caveats: docs/planning/gates/herdr-runner.md
# section 1. The job log is public: this prints no environment values.
set -u

expected_repo='RossGraeber/OAC'
expected_workflow='RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/main'

deny() {
  echo "oac-harness pre-job hook: refusing this job: $1" >&2
  exit 1
}

[ "${GITHUB_REPOSITORY-}" = "$expected_repo" ] || deny "repository is not $expected_repo"
case "${GITHUB_EVENT_NAME-}" in
  workflow_dispatch | push) ;;
  *) deny 'event is not workflow_dispatch or push' ;;
esac
[ "${GITHUB_WORKFLOW_REF-}" = "$expected_workflow" ] || deny 'workflow is not .github/workflows/herdr-provider-optin.yml at refs/heads/main'
[ "${GITHUB_REF-}" = 'refs/heads/main' ] || deny 'ref is not refs/heads/main'

echo 'oac-harness pre-job hook: allowed'
exit 0
