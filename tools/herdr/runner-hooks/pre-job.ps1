# Pre-job hook for the operator-owned `oac-harness` runners (Epic K, K6 #129), Windows.
#
# Same rules as pre-job.sh: GitHub runs this before any step of any job assigned to the
# runner when ACTIONS_RUNNER_HOOK_JOB_STARTED holds its absolute path. Exit 0 lets the job
# run; any other exit fails it before its first step. Default deny: only the opt-in
# workflow, from main, on a dispatch or a push, in this repository, gets through.
#
# Install it OUTSIDE the runner directory, writable only by Administrators (e.g.
# C:\ProgramData\oac-harness\pre-job.ps1), and set the variable at machine level, then
# restart the runner service. Setup and caveats: docs/planning/gates/herdr-runner.md
# section 1. The job log is public: this prints no environment values.
# Exercised by the K6 review under PowerShell 7 on Linux (12 cases, same results as
# pre-job.sh). Windows PowerShell 5.1 and any real runner: UNVERIFIED. Under the default
# Restricted execution policy on Windows client machines this cannot run and every job
# fails (fails closed): install PowerShell 7 or set RemoteSigned for the runner account.

$expectedRepo = 'RossGraeber/OAC'
$expectedWorkflow = 'RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/main'

function Deny([string] $why) {
  [Console]::Error.WriteLine("oac-harness pre-job hook: refusing this job: $why")
  exit 1
}

if ($env:GITHUB_REPOSITORY -cne $expectedRepo) { Deny "repository is not $expectedRepo" }
if (@('workflow_dispatch', 'push') -cnotcontains $env:GITHUB_EVENT_NAME) { Deny 'event is not workflow_dispatch or push' }
if ($env:GITHUB_WORKFLOW_REF -cne $expectedWorkflow) { Deny 'workflow is not .github/workflows/herdr-provider-optin.yml at refs/heads/main' }
if ($env:GITHUB_REF -cne 'refs/heads/main') { Deny 'ref is not refs/heads/main' }

Write-Output 'oac-harness pre-job hook: allowed'
exit 0
