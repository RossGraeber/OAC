# herdr opt-in runners (K6)

Epic K #123, K6 #129. The operator-owned self-hosted runners that
`.github/workflows/herdr-provider-optin.yml` runs on: what they need, what the workflow
does on them, what it publishes, and what it cannot protect against.

> **Status: UNVERIFIED — no runner is registered and no dispatch has run.** The workflow,
> `tools/herdr/ci.mjs` and this document were written and checked statically in a cloud
> session with no herdr binary, no logged-in harness and no self-hosted runner (actionlint
> 1.7.12 clean with `oac-harness` declared as a custom label;
> `node tools/herdr/run.mjs --self-test` and `node scripts/check-herdr-containment.mjs
> --self-test` pass). Nothing here has run on GitHub Actions, and the runner pre-job hook
> (§1) has not run on any runner. The first-dispatch and hook records at the end of this
> file are the operator's to fill in.

Every run this workflow makes is a scripted run under `oac-gates`
`references/scripted-runs.md`. It is **not verdict-bearing**, and it is the
provider-integration tier, opt-in by construction (`docs/planning/v0.1/09-test-strategy.md`
§4). It never changes a gate verdict, `docs/planning/STATUS.md` or `docs/planning/PINS.md`.

Sources for the GitHub facts below: `github/docs` at commit
`b5f08dd55f6576158f925235a30a5c4613baa6f6` (the docs.github.com source), retrieved
2026-09-28. Paths are given relative to https://github.com/github/docs/blob/b5f08dd55f6576158f925235a30a5c4613baa6f6/.

## 1. Read this first: this repository is public

`RossGraeber/OAC` is a public repository (GitHub API `visibility: public`, 2026-09-28).
GitHub's own guidance (`content/actions/reference/security/secure-use.md` L256-258):

> Self-hosted runners for GitHub do not have guarantees around running in ephemeral clean
> virtual machines, and can be persistently compromised by untrusted code in a workflow.
>
> As a result, self-hosted runners should almost never be used for public repositories on
> GitHub, because any user can open pull requests against the repository and compromise
> the environment.

These runners hold a logged-in harness. So read the next paragraph as the reason this
file exists, not as boilerplate.

**Why "no PR trigger in this workflow" is necessary but not enough.** The opt-in
workflow has no PR-event trigger (§3). That stops a fork from starting *this* workflow. It
does not stop a fork's pull request from editing *another* workflow. For a
`pull_request` event, GitHub runs the workflow files from the pull request's merge commit
(`content/actions/reference/workflows-and-actions/events-that-trigger-workflows.md`
L458-462: `GITHUB_SHA` is the "Last merge commit on the `GITHUB_REF` branch", `GITHUB_REF`
is `refs/pull/PULL_REQUEST_NUMBER/merge`). A fork can therefore change
`.github/workflows/boundary-lint.yml`, which runs on every pull request, to
`runs-on: [self-hosted, oac-harness, linux]` and run its own code on the runner.

**Fork-PR approval is not the protection.** GitHub's approval setting decides whose fork
pull requests run without a maintainer's click. GitHub itself says it is not a security
boundary for self-hosted runners (`data/reusables/actions/workflow-run-approve-public-fork.md`):

> These workflow approval policies are intended to restrict the set of users that can
> execute workflows in GitHub Actions runners that could lead to unexpected resource and
> compute consumption when using GitHub-hosted runners. If you are using self-hosted
> runners, potentially malicious user-controlled workflow code will execute automatically
> if the user is allowed to bypass approval in the set approval policy or if the pull
> request is approved. You must consider the risk of executing this code in your
> infrastructure and should review and follow the self-hosted runner security
> recommendations regardless of the approval settings utilized.

One approval of a fork pull request with a subtly changed `runs-on:` puts the fork's code
on the harness runner. Nothing in the repository can stop that, because the fork's
pull request brings its own copy of every workflow and of every lint.

**The protection is a pre-job hook on the runner machine.** The runner runs a script
named by `ACTIONS_RUNNER_HOOK_JOB_STARTED` "when a job has been assigned to a runner, but
before the job starts running". A non-zero exit means "the job will not run and will be
marked as failed" (`content/actions/how-tos/manage-runners/self-hosted-runners/run-scripts.md`
L46, L57-58). The script sees the job's default variables (same file, "Writing the
scripts"). `GITHUB_WORKFLOW_REF` is "the ref path to the workflow", for example
`octocat/hello-world/.github/workflows/my-workflow.yml@refs/heads/my_branch`
(`data/reusables/actions/workflow-ref-description.md`). The hook lives on the runner
machine, so no pull request and no workflow change can edit it.
`tools/herdr/runner-hooks/pre-job.sh` (Linux) and `pre-job.ps1` (Windows) allow a job
only when **all** of these hold, and refuse everything else:

- `GITHUB_REPOSITORY` is `RossGraeber/OAC`;
- `GITHUB_EVENT_NAME` is `workflow_dispatch` or `push`;
- `GITHUB_WORKFLOW_REF` is exactly
  `RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/main`;
- `GITHUB_REF` is `refs/heads/main`.

So a fork pull request is refused, whichever workflow it edits and whether or not it was
approved: its event is `pull_request*` and its workflow ref is `refs/pull/<n>/merge`. So is
a dispatch from any branch but `main`, by anyone with write access. What the hook admits
is only the opt-in workflow as committed on `main`, so the trust boundary becomes merge
rights on `main`.

**Hook status: UNVERIFIED.** The behavior above is GitHub's documented behavior
(`run-scripts.md` at the cited commit). The bash hook passes its self-test cases
(`node tools/herdr/run.mjs --self-test`, "hook: …": the allowed dispatch and push, and
refusals of fork pull requests, `pull_request_target`, an `issues` event, a dispatch from
another branch, another workflow, another repository, and missing variables). The K6
review ran `pre-job.ps1` under PowerShell 7 (`pwsh`) on Linux across 12 cases, with the same
results as the bash hook (2 allowed, 10 refused). That run was the reviewer's, not
reproduced in the session that wrote this file. Windows PowerShell 5.1, the Windows default,
is untested. Under the default `Restricted` execution policy on Windows client machines the
hook cannot run, so every job fails: the hook fails closed (a denial of service), not open.
Install PowerShell 7, or set the `RemoteSigned` policy for the runner's account. Neither
script has run on a real runner. That the variable values arrive with exactly this
spelling and casing is also unobserved.
After installing, check a refused and an allowed job's `Set up runner` log section, where
GitHub shows the hook's output (`run-scripts.md` L89).

**Installing the hook (operator):**

- Linux: copy `tools/herdr/runner-hooks/pre-job.sh` to a path outside the runner
  directory that the runner user cannot write (for example root-owned
  `/usr/local/lib/oac-harness/pre-job.sh`, mode 0755). GitHub says the scripts "should not
  be stored in the `actions-runner` application directory" (`run-scripts.md`, "Triggering
  the scripts"). Set `ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/lib/oac-harness/pre-job.sh`.
  GitHub allows the variable "in the operating system" or in the runner directory's
  `.env` file. Prefer the service's own root-owned environment (for example a systemd
  drop-in), because the runner directory, `.env` included, is writable by the runner
  user. Restart the runner. This does not stop the runner's own `.env` from overriding
  the variable (see the residual risk below).
- Windows: copy `pre-job.ps1` to a directory only Administrators can write (for example
  `C:\ProgramData\oac-harness\`), set `ACTIONS_RUNNER_HOOK_JOB_STARTED` as a machine-level
  environment variable, and restart the runner service.
- Residual risk: the hook keeps jobs it does not admit from starting. It cannot survive a
  compromised job that it does admit. At startup the runner reads `<runner root>/.env` and
  sets each line as an environment variable, overriding the service environment. An empty
  value unsets the variable (actions/runner `src/Runner.Listener/Program.cs`
  `LoadAndSetEnv()`, commit `15231bede4aacecb6686f4b7de25c62398607993`, retrieved
  2026-09-28). So any process running as the runner user can write
  `ACTIONS_RUNNER_HOOK_JOB_STARTED=` into `.env` and disable the hook for every later job,
  across restarts. That includes an admitted job that has been prompt-injected or
  otherwise compromised. In a standard install the runner user owns the runner directory,
  and this guidance does not change that. Keeping the hook script unwritable prevents
  editing the script, not unsetting the variable. The severity is persistence, not new
  access: such a job already runs as the user holding the harness login. Closing it would
  need the runner directory (at least `.env`) to be unwritable by the runner user, which
  is not verified to work with the runner's self-update.

**Also required before registering any runner (operator), as defense in depth:**

1. Settings → Actions → General → "Approval for running fork pull request workflows from
   contributors": choose **"Require approval for all external contributors"**. The two
   first-time-contributor options can be bypassed by getting "a simple typo or other
   innocuous change accepted by a maintainer"
   (`data/reusables/actions/workflows-from-public-fork-setting.md`). This reduces how
   often fork code runs anywhere. It does not protect the runner (quote above).
2. Read a fork pull request's `.github/` diff before approving its workflow run.
   `scripts/check-herdr-containment.mjs` (check 9) fails any workflow other than the
   opt-in one that names a label a self-hosted runner carries. That includes the default
   OS and architecture labels, since GitHub routes a job to any runner that has all of
   its `runs-on` labels. But the lint runs from the fork's own copy, so it catches drift
   in this repository, not a hostile fork.
3. Keep write access to people you trust with the runner machine. Write access can
   dispatch against any branch or tag (`events-that-trigger-workflows.md` L1079: "Once a
   workflow has run at least once, you can dispatch it against any branch or tag via the
   GitHub API or GitHub CLI"). The hook refuses such a dispatch; the workflow's own
   `if: github.ref == 'refs/heads/main'` only guards against mistakes, since a dispatched
   ref runs its own copy of the file.
4. Protect `main` (require review before merge). With the hook in place, merge rights on
   `main` are the trust boundary: whoever can change `main` can change the workflow and
   the driver that run on the runner.

Runner groups, which can restrict which repositories or workflows reach a runner
(`secure-use.md` L262), belong to organizations and enterprises. This repository is owned
by a personal account, so they are not available here. If the repository moves to an
organization, put these runners in their own group restricted to this workflow file, in
addition to the hook.

## 2. Runner prerequisites

| Requirement | Detail |
|---|---|
| Labels | Linux: `self-hosted`, `oac-harness`, `linux`. Windows: `self-hosted`, `oac-harness`, `windows`. `self-hosted` and the OS label are default labels GitHub applies to a new self-hosted runner (`content/actions/how-tos/manage-runners/self-hosted-runners/use-in-a-workflow.md` L40-46). Add `oac-harness` when you configure the runner: `--labels oac-harness` on `config.sh`/`config.cmd` (`apply-labels.md` L131-141). Labels given that way are not validated against the machine (same file, L136), so check the OS label is right. No other runner may carry `oac-harness`. The runners keep their default labels, because the workflow's `runs-on` names `self-hosted` and the OS label (K6 acceptance). As a result, any job whose `runs-on` is a subset such as `[linux, x64]` could be routed to them. Check 9 therefore fails any other workflow that names `self-hosted`, `oac-harness`, `linux`, `windows`, `macos`, `x64`, `arm` or `arm64`, and the pre-job hook refuses such a job anyway. |
| Pre-job hook | **Required.** `tools/herdr/runner-hooks/pre-job.sh` or `pre-job.ps1`, installed as §1 describes. |
| Runner software | Actions Runner **v2.327.1 or later**. Both pinned actions run on `node24` (`action.yml` of actions/checkout `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1) and actions/upload-artifact `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` (v7.0.1)). actions/checkout's README at that commit: "This requires a minimum Actions Runner version of v2.327.1 to run." |
| Dedicated OS user | Run the runner service as an OS user that is used for nothing else, on a machine that holds no other credentials (SSH keys, cloud credentials; `secure-use.md` L264-267). Everything that user can read, a job can read. |
| herdr | Exactly the `docs/planning/PINS.md` row `herdr (test tooling)`: **`v0.9.1`** at K6 (tag `v0.9.1` → commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`). `herdr --version` must print `herdr 0.9.1`, or the driver ends the run `NOT RUN` before it starts a session. On `PATH` for the runner user. Never run `herdr integration install`, and do not run `herdr update`: a new herdr version is a PINS.md pin move first. |
| Node.js | On `PATH` for the runner user. The driver uses Node built-ins only and installs nothing; the cloud self-test ran on Node v22.22.2. The workflow does not use `setup-node`, a cache action or `npm`. |
| git | On `PATH`. The driver records `git HEAD` and whether `tools/herdr/` is dirty. |
| Harness login | **The operator logs in by hand, once, as the runner user, outside CI.** For `g1-claude-wake`: Claude Code at any version. Versions float (#216, 2026-10-01): a version other than the one PINS.md records as last tested for `Claude Code (Channels)`, or one below its minimum, is a `VERSION WARNING` finding in the run manifest. It never stops the run, never fails the job and edits nothing in PINS.md. (Until 2026-10-01 such a run stopped `NOT RUN` with a pin-move trigger.) Log in with Claude Code's own interactive login. The workflow never logs in, never reads, copies or uploads a credential, and never writes harness config. The driver hashes harness config before and after each run and never writes it (K3; `oac-boundaries` check 10). |
| No API keys in the runner environment | Do not set a provider API key or harness token in the runner's service environment or its `.env` file. Every job step would see it, and the harness would stop being the operator's own login session. The workflow references no secret and no key. |
| Codex | Not needed at K6 (no Codex scenario is allowlisted; K7 adds one). |

On Windows, herdr's support is herdr's own documentation claim only
(`docs/planning/decisions/K1-herdr-evaluation.md`; `docs/planning/STATUS.md` K1 entry). The
driver's own self-test runs its lifecycle half on POSIX only. A Windows runner is
therefore doubly UNVERIFIED until its first dispatch.

## 3. What the workflow does

**Triggers.** Only these two:

- `workflow_dispatch`, with inputs `scenario` (choice: `smoke`, `g1-claude-wake`; default
  `smoke`) and `runner` (choice: `both`, `linux`, `windows`; default `both`);
- `push` to `main` that changes `docs/planning/PINS.md`. That run uses `smoke` on both
  runners. It checks that herdr at the (possibly new) pin still starts, reads a pane and
  tears down. It does not run G1, which needs the operator at the keyboard.

There is no other trigger. `scripts/check-herdr-containment.mjs` check 9 reads the
workflow's `on:` keys and fails the build unless they are exactly `workflow_dispatch` and
`push`, with `push` limited to `main` and `docs/planning/PINS.md`. It is an allowlist, so
a trigger any GitHub user can fire (`issues`, `watch`, `fork`, `discussion`, …) fails too.
The same check requires `permissions` to be exactly `contents: read`, every `uses:` to be
`actions/checkout` or `actions/upload-artifact` pinned by commit SHA, checkout to set
`persist-credentials: false`, and forbids any `secrets` or `github.token` use (see
`.claude/skills/oac-boundaries/references/mechanical-checks.md`). No other workflow runs
the driver or names a label these runners carry (check 9 again). The pre-job hook (§1)
refuses any other event or workflow on the runner itself, whatever the files say. The default CI tier,
`boundary-lint.yml` on `ubuntu-latest`, is unchanged. It runs the containment lint, which
mentions herdr only in its script name, and never runs the driver or herdr.

**Concurrency.** One job at a time per runner label: group
`herdr-provider-optin-<linux|windows>`, `cancel-in-progress: false`. A run in progress is
never cancelled by a newer one. GitHub keeps at most one run pending per group, and a newer
queued run replaces an older pending one
(`data/reusables/actions/actions-group-concurrency.md`). Two dispatches in quick succession
can therefore drop the older pending one. Re-dispatch it if you need it.

**Steps**, all through `tools/herdr/ci.mjs`:

1. `actions/checkout` (pinned by commit SHA, `persist-credentials: false`, so no token is
   written into the checkout; `clean: true` is its default and empties the workspace
   first).
2. `ci.mjs run`. It reads the scenario only from `OAC_HERDR_SCENARIO` (set through `env:`,
   never spliced into the command) and checks it against the allowlist `smoke`,
   `g1-claude-wake`. A path, a flag or any other name is refused. It removes CI job tokens
   (`ACTIONS_*`, `GITHUB_TOKEN`, `GH_TOKEN`) and the workflow-command file paths
   (`GITHUB_ENV`, `GITHUB_OUTPUT`, `GITHUB_PATH`, `GITHUB_STATE`, `GITHUB_STEP_SUMMARY`)
   from the driver's environment. It points `TMPDIR`/`TEMP`/`TMP` at
   `$RUNNER_TEMP/oac-herdr-<run id>-<attempt>/tmp`, so the driver's scratch directory
   lives inside the job's temp directory. Then it runs the driver with the scenario's own
   defaults. It passes no other driver option, and it never retries.
3. `ci.mjs stage` (runs unless the job was cancelled). The upload gate, below.
4. `actions/upload-artifact` (pinned by commit SHA), only if step 3 succeeded. Artifact
   `herdr-<os>-<run id>-<attempt>`, 14-day retention, hidden files excluded.
5. `ci.mjs cleanup` (always runs). It deletes the job's work dir, scratch included, and
   fails if the run left any changed, untracked or ignored path in the checkout.

The job's outcome follows the driver's exit code: `PASS` 0; `FAIL` 1; `NOT RUN` 3, which
also fails the job. A red job is not a gate finding by itself. Read the run manifest.

**`g1-claude-wake` runs unattended in CI (amended 2026-09-30, #196).** CI passes no driver
option, so the scenario runs on its defaults. Under the operator's second decision on #196,
that default is `accept=driver`.

- **What the driver accepts.** It accepts the folder-trust, project-MCP-server and
  `--dangerously-load-development-channels` dialogs itself, each only when it matches the
  recorded text, and records each accept as `driver`.
- **What it refuses.** Any other dialog, for example a tool-permission prompt. The run then
  ends `NOT RUN` with no key sent.
- **What the run cannot be.** G1 criterion 5 is the dev-channels consent step itself, so on a
  CI G1 run it is `not evaluable`, and the run is never a G1 equivalence record
  (`scripted-runs.md` "Operator-consent dialogs").
- **A human-accepted G1 run** (`--param accept=human`) cannot come from CI, because CI cannot
  pass a parameter. Run it locally. The driver then sends nothing, and you accept each dialog
  yourself at the pane (`herdr session attach <session>`). For trust and MCP, move the
  selection to "Yes, I trust this folder" or "Use this MCP server" before Enter. The accept
  counts only if the record names you, the person who pressed it, under "Human actions" in
  its Verification section (#252).

herdr keeps a named session's socket under the runner user's config directory
(`$XDG_CONFIG_HOME` or `~/.config/herdr/sessions/<name>/` on Linux,
`%APPDATA%\herdr\sessions\<name>\` on Windows). It is not under `TMPDIR`, so the job's
`TMPDIR` redirect does not hide the session from your shell. That comes from reading the
source at `v0.9.1` (`src/config/io.rs` `config_dir`/`platform_config_dir`,
`src/session.rs` `api_socket_path_for`) and is UNVERIFIED live.

## 4. What is uploaded, and the gate before it

The artifact holds only what the driver's `--out` directory holds after redaction: the
redacted `run-manifest.json` and the redacted captures (`pane-*.txt`, `transcript-*.jsonl`).
It never holds the raw scratch directory, the herdr server log, the herdr config, raw
unredacted output, or anything from the harness's home or config.

`ci.mjs stage` stages files for upload only if **all** of these hold. Otherwise it
stages nothing, the upload step is skipped, and the job fails:

- `run-manifest.json` exists as a regular file, and its `manifestRedaction.writtenClean`
  is `true`;
- every capture the manifest lists was written (the driver withheld none), has an
  allowlisted name (`pane-….txt` / `transcript-….jsonl`), exists as a regular file (not a
  symlink), and has a recorded redaction report with zero `residualLeaks`, zero
  `residualGenericHits` and zero `hazardProtocolFrames` (K3's `tools/herdr/lib/redact.mjs`);
- an independent re-scan of every file to be staged finds no residual hit. It uses the
  runner machine's own home path, user name and host name, plus the checkout path and the
  job's work-dir path.

Anything else in the `--out` directory is left behind and deleted by `cleanup`. The step
summary shows the scenario, the run outcome (labelled "not a gate verdict"), and which
files were staged or why none were.

**Who can see it.** This is a public repository. Its Actions logs are public, and its
artifacts can be downloaded by anyone who can read the repository. That is why the gate
fails closed, and why `ci.mjs run` prints the outcome from the redacted manifest, never
from the driver's in-memory record.

**Committing a run as evidence stays a separate, verified step.** Download the artifact.
For G1, run `node tools/herdr/lib/g1-report.mjs --run <downloaded dir>`, then the rest of
`scripted-runs.md` "Checklist: recording a scripted run". That g1-report accepts a
downloaded artifact directory as `--run` is UNVERIFIED. A record or fixture from this
workflow needs the `## Verification` section that `node scripts/check-fixture-manifest.mjs`
requires, re-checked by the recording agent against the committed run manifest and
captures (#252; before #252 this was an operator attestation). **The workflow never writes
or fills a Verification section, and must not.** A job cannot check its own evidence: the
check happens after the run, outside the job, from what the run manifest recorded (the
herdr hash against PINS.md, the harness versions, every dialog accept), and anything it
cannot show stays UNVERIFIED (`scripted-runs.md` "Driver identity", "Verification").

## 5. What stays on the runner

- **Job work dir** (`$RUNNER_TEMP/oac-herdr-<run id>-<attempt>`: driver temp and scratch,
  `--out`, upload staging) is deleted by `cleanup`. The runner also empties `RUNNER_TEMP`
  "at the beginning and end of each job"
  (`data/reusables/actions/runner-temp-directory-description.md`). Only a run killed hard
  (a third signal to the driver, or the runner killing the process tree) can leave the
  herdr session or scratch behind until the next job starts.
- **Checkout** stays in the runner's `_work` directory: public repository content, with no
  token persisted. The next job's checkout cleans it.
- **Harness state.** Accepting Claude Code's folder-trust or project-MCP-server dialog for
  the scratch project directory makes Claude Code record that trust in its own state. The
  driver never writes it (`tools/herdr/scenarios/g1-claude-wake.mjs` header).
- **herdr session directory** under the runner user's herdr config directory: the driver
  stops and deletes the session at teardown and records whether teardown was clean.
- **Runner diagnostic logs** (`_diag`) hold the job log, which is public anyway.
- No npm cache, tool cache or action cache is created by this workflow.

## 6. Threat table

Template: `oac-security-work` §1. A row without a proving test is an open risk, recorded
as such in `docs/planning/v0.1/11-risks.md` row 52 and `docs/planning/STATUS.md`.

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| A fork's pull request edits any workflow (for example `boundary-lint.yml`'s `runs-on:`) to run on a harness runner and reads the logged-in harness's credentials | A fork workflow run proceeds: the contributor bypasses approval, or a maintainer approves it | **Pre-job hook** on the runner refuses every job that is not `.github/workflows/herdr-provider-optin.yml@refs/heads/main` in `RossGraeber/OAC` on `workflow_dispatch`/`push` (§1). Defense in depth: fork approval for all external contributors, reading `.github/` diffs, and check 9 on other workflows' runner labels (drift only) | Hook logic: `run.mjs --self-test` "hook: refuses a fork PR through another workflow", "… through an edited opt-in workflow", "… pull_request_target on main", "… an issues event on main". Check 9: "9 other workflow runs on bare default labels", "… targets the oac-harness label", "… targets a self-hosted runner". **Not proven on a real runner** | **Open risk until the hook is observed refusing a job on each runner** (UNVERIFIED, §1). After that: a compromised admitted job can disable the hook for later jobs by writing `ACTIONS_RUNNER_HOOK_JOB_STARTED=` into the runner's own `.env`, which overrides the service environment (persistence, not new access; §1); a GitHub change to hook behavior or variable values |
| A fork's pull request, or any GitHub user, triggers the opt-in workflow directly | A PR-event, chained, or stranger-fireable trigger (`issues`, `watch`, `fork`, `discussion`, …) is added to it | Triggers are an allowlist: check 9 fails unless `on:` is exactly `workflow_dispatch` + `push`, with `push` limited to `main` and `docs/planning/PINS.md`; the hook refuses any other event | `check-herdr-containment.mjs --self-test`: "9 opt-in workflow gains a PR-event trigger", "… an issues / watch / fork / discussion trigger", "… a chained trigger", "… a schedule", "… trigger list form …", "… push widened …", "… push gains tags"; hook "refuses an issues event on main" | A trigger added and merged by someone with merge rights, together with a lint change; the hook still refuses the event |
| A collaborator dispatches the workflow on a branch carrying modified driver or workflow code | Write access | Pre-job hook refuses any workflow ref but `…@refs/heads/main`; write access limited to trusted people; the workflow's own `if: github.ref == 'refs/heads/main'` (mistakes only) | `run.mjs --self-test` "hook: refuses a dispatch from another branch". Not proven on a real runner | Until the hook is observed live: **open risk**, bounded by who holds write access |
| A crafted `docs/planning/PINS.md` merged to `main` injects commands when the push trigger fires | Merge to `main` | PINS.md is only a `paths:` filter: no workflow expression reads it, no expression appears in any `run:` block, and the driver parses it with a strict table parser (`tools/herdr/lib/pins.mjs`) that only extracts a `vX.Y.Z` tag and compares it with `herdr --version`. A mismatch is `NOT RUN` | `check-herdr-containment.mjs --self-test` ("expression in an inline run", "… in a block run"); `run.mjs --self-test` pin cases | Whoever can merge to `main` can edit the workflow or driver directly, so the boundary is merge rights, not PINS.md |
| Expression injection through the dispatch input | Write access (dispatch) | `choice` input; passed only through `env:`; `ci.mjs` allowlist refuses paths, flags and unknown names | `run.mjs --self-test`: "ci: scenario … is refused" (8 cases); check 9 run-block cases | None known |
| CI runs a scenario with a policy or parameters other than its committed defaults (amended 2026-09-30, #196: CI's G1 run now driver-accepts the three recorded dialogs by the scenario's own default, an operator decision; criterion 5 is then `not evaluable`, and anything else, a tool-permission prompt included, is refused and the run is `NOT RUN`) | A driver option such as `--param`, `--launch` or `accept=…` reaching the driver | `ci.mjs` passes no driver option; check 9 fails the workflow if it names a driver option or calls `run.mjs`; the driver refuses every dialog kind with no option text on record | check-herdr self-test "9 opt-in workflow passes accept=driver", "… calls the driver directly"; ci self-test "g1-claude-wake --param accept=driver is refused"; `run.mjs --self-test` "g1 #197 tool permission: default policy is driver, run NOT RUN, no dialog-accept command" | A change to a scenario's committed default goes through review, not the workflow |
| Credentials or identifying data published in the artifact | Redaction misses a value, or a surviving process rewrites a file after the scan | K3 redaction; the driver withholds unclean captures; `ci.mjs stage` fails closed on any recorded or re-scanned residual and on a teardown that was not clean; it writes to `upload/` exactly the bytes it scanned; name allowlist; symlinks refused | `run.mjs --self-test`: "ci stage: …" (20 cases, including "a run whose teardown was not clean stages nothing" and "stages the bytes it scanned, not a later re-read"), "ci lifecycle: …", and K3's redaction cases | Redaction is pattern-based: a secret with no known shape and no known literal can pass (K3 limits). The upload step reads `upload/` later, so a process that survives despite a clean teardown record could still change it |
| The harness under test, or herdr, is steered into reading the job token or writing `GITHUB_ENV` to influence later steps | Prompt injection into the harness during a run | Check 9 fails any `secrets` use (`secrets.X`, `secrets[…]`, `toJSON(secrets)`, `secrets: inherit`) and any `github.token`; tokens, token-shaped values under any name, and workflow-command file paths are removed from the driver's environment; `permissions` is exactly `contents: read`; scenario prompts are fixed; later steps run only `ci.mjs` and pinned actions | `check-herdr-containment.mjs --self-test` "9 opt-in workflow reads toJSON(secrets)", "… reads github.token", "… reads a secret", "… sets contents: write", "… adds id-token: write", "… uses write-all", "… adds job-level permissions"; `run.mjs --self-test` "ci: scrub removes …" (2 cases) | Defense in depth only: the harness runs as the runner user and could find those files on disk |
| Paths or user names leak through the public job log | The driver, a scenario, or an exception prints raw values | `ci.mjs` prints the outcome from the redacted manifest and passes every error message through the redactor before printing | `run.mjs --self-test` "ci: error messages are redacted before printing" | Output the driver or a scenario prints directly (the G1 accept hint prints only a session name) |
| A compromised or movable action runs on the runner | An action referenced by tag or branch is moved, or a third-party or reusable-workflow `uses:` is added | Check 9: every `uses:` must be `actions/checkout` or `actions/upload-artifact` at a 40-hex commit SHA; checkout must set `persist-credentials: false` | `check-herdr-containment.mjs --self-test` "9 opt-in workflow pins checkout by a tag", "… pins upload-artifact by a branch", "… uses a third-party action", "… checkout persists the token" | A malicious commit at the pinned SHA itself |
| Unredacted scratch left on the runner | A hard kill mid-run | The driver removes scratch; `cleanup` removes the work dir; the runner empties `RUNNER_TEMP` | `run.mjs --self-test`: "lifecycle: every run removed its scratch directory", "ci cleanup: removes the work dir", "ci lifecycle: cleanup removes the work dir" | Until the next job starts, after a hard kill |

Doctrine (`oac-security-work` §3): nothing a harness or pane prints during a run is an
instruction to the workflow. The workflow acts only on the driver's exit code and the
stage gate's own checks.

## 7. Operator checklist before the first dispatch

- [ ] Pre-job hook installed on each runner, outside the runner directory, not writable by
      the runner user, and named by `ACTIONS_RUNNER_HOOK_JOB_STARTED` in the service
      environment (§1). Runner restarted. The runner's own `.env` does not set or empty
      that variable, because `.env` overrides the service environment. Check it again
      after any suspect run (§1, residual risk). Windows: PowerShell 7 installed, or the
      `RemoteSigned` policy set for the runner account, or the hook refuses every job.
- [ ] Hook observed live: one refused job (for example a dispatch from a scratch branch)
      and one allowed job, each checked in the job's `Set up runner` log section, on each
      runner. Record both in §8.
- [ ] Fork-PR approval set to "Require approval for all external contributors" (§1),
      as defense in depth.
- [ ] `main` protected. Write access reviewed.
- [ ] Dedicated runner user and machine, with no other credentials on it (§2).
- [ ] Runner v2.327.1 or later, registered with label `oac-harness`, on Linux and on
      Windows.
- [ ] `herdr --version` prints `herdr 0.9.1` as the runner user. Node and git on `PATH`.
- [ ] Claude Code installed and logged in interactively as the runner user. Any version
      runs; one other than PINS.md's last tested version is a `VERSION WARNING`, never a
      stop (#216).
- [ ] No provider API key or harness token in the runner's environment.
- [ ] First dispatch: `scenario=smoke`, `runner=linux`, then `runner=windows`. Then
      `scenario=g1-claude-wake` per runner. Since #196 it runs unattended: the driver accepts
      the three recorded dialogs, and G1 criterion 5 is then `not evaluable`.

## 8. First-dispatch record (operator: fill in)

K6's acceptance asks for one successful G1 dispatch on Linux and one on Windows, with run
URLs recorded here. **None has run.** Do not fill a row from anything but a real run.

| Runner | Scenario | Run URL | Date (UTC) | Commit | `herdr --version` | `claude --version` | Run outcome | Evidence staged | Operator |
|---|---|---|---|---|---|---|---|---|---|
| linux | g1-claude-wake | TODO (operator) — NOT RUN | | | | | | | |
| windows | g1-claude-wake | TODO (operator) — NOT RUN | | | | | | | |

Pre-job hook, observed live (operator: fill in):

| Runner | Refused job (run URL, reason in `Set up runner`) | Allowed job (run URL) | Date (UTC) | Operator |
|---|---|---|---|---|
| linux | TODO (operator) — NOT RUN | | | |
| windows | TODO (operator) — NOT RUN | | | |

A run outcome of `PASS` here means the driver ran the scenario to the end. It is not a G1
verdict (`scripted-runs.md` "Three vocabularies").
