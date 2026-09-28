# herdr opt-in runners (K6)

Epic K #123, K6 #129. The operator-owned self-hosted runners that
`.github/workflows/herdr-provider-optin.yml` runs on: what they need, what the workflow
does on them, what it publishes, and what it cannot protect against.

> **Status: UNVERIFIED — no runner is registered and no dispatch has run.** The workflow,
> `tools/herdr/ci.mjs` and this document were written and checked statically in a cloud
> session with no herdr binary, no logged-in harness and no self-hosted runner (actionlint
> 1.7.12 clean with `oac-harness` declared as a custom label;
> `node tools/herdr/run.mjs --self-test` and `node scripts/check-herdr-containment.mjs
> --self-test` pass). Nothing here has run on GitHub Actions. The first-dispatch record
> at the end of this file is the operator's to fill in.

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
`runs-on: [self-hosted, oac-harness, linux]` and run its own code on the runner. GitHub
does not refuse this for self-hosted runners on its own. The protection is a repository
setting that the owner must turn on.

**Required before registering any runner (operator):**

1. Settings → Actions → General → "Approval for running fork pull request workflows from
   contributors": choose **"Require approval for all external contributors"**. The two
   first-time-contributor options are not enough. GitHub warns
   (`data/reusables/actions/workflows-from-public-fork-setting.md`): "a user that has had
   any commit or pull request merged into the repository will not require approval. A
   malicious user could meet this requirement by getting a simple typo or other innocuous
   change accepted by a maintainer."
2. Never approve a fork pull request's workflow run until you have read its diff under
   `.github/`. `scripts/check-herdr-containment.mjs` (check 9) fails a workflow other than
   the opt-in one that names `oac-harness` or `self-hosted`. But it runs from the fork's
   own copy, alongside the fork's jobs, so it catches drift in this repository and cannot
   stop a hostile fork.
3. Keep write access to this repository to people you trust with the runner machine.
   Write access can dispatch the workflow against any branch or tag
   (`events-that-trigger-workflows.md` L1079: "Once a workflow has run at least once, you
   can dispatch it against any branch or tag via the GitHub API or GitHub CLI"). A
   dispatched ref runs its own copy of the workflow file, so the job's
   `if: github.ref == 'refs/heads/main'` guards against mistakes only.
4. Protect `main` (require review before merge). Merge rights are the real trust boundary:
   whoever can change `main` can change the workflow and the driver that run on the
   runner.

Runner groups, which can restrict which repositories or workflows reach a runner
(`secure-use.md` L262), belong to organizations and enterprises. This repository is owned
by a personal account, so they are not available here. If the repository moves to an
organization, put these runners in their own group restricted to this workflow file.

## 2. Runner prerequisites

| Requirement | Detail |
|---|---|
| Labels | Linux: `self-hosted`, `oac-harness`, `linux`. Windows: `self-hosted`, `oac-harness`, `windows`. `self-hosted` and the OS label are default labels GitHub applies to a new self-hosted runner (`content/actions/how-tos/manage-runners/self-hosted-runners/use-in-a-workflow.md` L40-46). Add `oac-harness` when you configure the runner: `--labels oac-harness` on `config.sh`/`config.cmd` (`apply-labels.md` L131-141). Labels given that way are not validated against the machine (same file, L136), so check the OS label is right. No other runner may carry `oac-harness`. |
| Runner software | Actions Runner **v2.327.1 or later**. Both pinned actions run on `node24` (`action.yml` of actions/checkout `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1) and actions/upload-artifact `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` (v7.0.1)). actions/checkout's README at that commit: "This requires a minimum Actions Runner version of v2.327.1 to run." |
| Dedicated OS user | Run the runner service as an OS user that is used for nothing else, on a machine that holds no other credentials (SSH keys, cloud credentials; `secure-use.md` L264-267). Everything that user can read, a job can read. |
| herdr | Exactly the `docs/planning/PINS.md` row `herdr (test tooling)`: **`v0.9.1`** at K6 (tag `v0.9.1` → commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`). `herdr --version` must print `herdr 0.9.1`, or the driver ends the run `NOT RUN` before it starts a session. On `PATH` for the runner user. Never run `herdr integration install`, and do not run `herdr update`: a new herdr version is a PINS.md pin move first. |
| Node.js | On `PATH` for the runner user. The driver uses Node built-ins only and installs nothing; the cloud self-test ran on Node v22.22.2. The workflow does not use `setup-node`, a cache action or `npm`. |
| git | On `PATH`. The driver records `git HEAD` and whether `tools/herdr/` is dirty. |
| Harness login | **The operator logs in by hand, once, as the runner user, outside CI.** For `g1-claude-wake`: Claude Code at the version PINS.md records as last observed for `Claude Code (Channels)`; otherwise the scenario stops the run `NOT RUN` (pin-move trigger, no PINS.md edit). Log in with Claude Code's own interactive login. The workflow never logs in, never reads, copies or uploads a credential, and never writes harness config. The driver hashes harness config before and after each run and never writes it (K3; `oac-boundaries` check 10). |
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

There is no PR-event trigger, no trigger chained from another workflow or from a comment,
and no schedule. `scripts/check-herdr-containment.mjs` check 9 fails the build if one is
added (see `.claude/skills/oac-boundaries/references/mechanical-checks.md`). No other
workflow runs the driver or targets these runners (check 9 again). The default CI tier,
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

**`g1-claude-wake` needs the operator.** The scenario keeps its default `accept=human`,
and CI cannot change it. The `--dangerously-load-development-channels` confirmation is
never accepted by the driver (`scripted-runs.md` "Operator-consent dialogs"). When the
dialog is on screen, the job log prints the herdr session name and a command:
`herdr --session <name> agent send-keys g1claude enter`. Run it yourself as the runner
user on the runner machine within `humanAcceptTimeoutMs` (5 minutes by default), or the
run ends `NOT RUN`. herdr keeps a named session's socket under the runner user's config
directory (`$XDG_CONFIG_HOME` or `~/.config/herdr/sessions/<name>/` on Linux,
`%APPDATA%\herdr\sessions\<name>\` on Windows). It is not under `TMPDIR`, so the job's
`TMPDIR` redirect does not hide the session from your shell. That comes from reading the
source at `v0.9.1` (`src/config/io.rs` `config_dir`/`platform_config_dir`,
`src/session.rs` `api_socket_path_for`) and is UNVERIFIED live. By the rule in
`scripted-runs.md`, the accept counts only if you, the person who pressed it, attest to it.

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

**Committing a run as evidence stays a manual, attested step.** Download the artifact.
For G1, run `node tools/herdr/lib/g1-report.mjs --run <downloaded dir>`, then the rest of
`scripted-runs.md` "Checklist: recording a scripted run". That g1-report accepts a
downloaded artifact directory as `--run` is UNVERIFIED. A record or fixture from this
workflow needs the `## Operator attestation` section that
`node scripts/check-fixture-manifest.mjs` requires, written by the operator who ran the
machine. **The workflow never writes an attestation, and must not.** An attestation exists
precisely because automation cannot prove that it ran the real herdr and the real harness
rather than a test double (`scripted-runs.md` "Driver identity").

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
| A fork's pull request edits an existing PR-triggered workflow (`boundary-lint.yml`) to run on `oac-harness` and reads the logged-in harness's credentials | Fork workflow runs do not require approval for that contributor, or a maintainer approves without reading `.github/` | Repository setting "Require approval for all external contributors" (§1); maintainers read `.github/` diffs before approving; check 9 fails a non-opt-in workflow naming `oac-harness`/`self-hosted` (drift only) | **None for the setting**: it is an operator-verified repository setting, not testable from the repository. Check 9 drift: `check-herdr-containment.mjs --self-test` cases "9 other workflow targets …" | **Open risk.** GitHub's own guidance is that self-hosted runners should almost never be used with public repositories; one careless approval compromises the runner persistently |
| A fork's pull request triggers the opt-in workflow directly | A PR-event or chained trigger is added to it | No such trigger; check 9 fails one being added | `check-herdr-containment.mjs --self-test`: "9 opt-in workflow gains a PR-event trigger", "… a chained trigger", "… a schedule" | A trigger added and merged by someone with merge rights, together with a lint change |
| A collaborator dispatches the workflow on a branch carrying modified driver or workflow code | Write access | Write access limited to trusted people; `if: github.ref == 'refs/heads/main'` (mistakes only) | None: the dispatched ref runs its own workflow copy | **Open risk**, bounded by who holds write access |
| A crafted `docs/planning/PINS.md` merged to `main` injects commands when the push trigger fires | Merge to `main` | PINS.md is only a `paths:` filter: no workflow expression reads it, no expression appears in any `run:` block, and the driver parses it with a strict table parser (`tools/herdr/lib/pins.mjs`) that only extracts a `vX.Y.Z` tag and compares it with `herdr --version`. A mismatch is `NOT RUN` | `check-herdr-containment.mjs --self-test` ("expression in an inline run", "… in a block run"); `run.mjs --self-test` pin cases | Whoever can merge to `main` can edit the workflow or driver directly, so the boundary is merge rights, not PINS.md |
| Expression injection through the dispatch input | Write access (dispatch) | `choice` input; passed only through `env:`; `ci.mjs` allowlist refuses paths, flags and unknown names | `run.mjs --self-test`: "ci: scenario … is refused" (8 cases); check 9 run-block cases | None known |
| CI auto-accepts the `--dangerously-load-development-channels` confirmation | A driver option such as `accept=driver` reaching the driver | `ci.mjs` passes no driver option; check 9 fails the workflow if it names a driver option or calls `run.mjs` | check-herdr self-test "9 opt-in workflow passes accept=driver", "… calls the driver directly"; ci self-test "g1-claude-wake --param accept=driver is refused" | None known |
| Credentials or identifying data published in the artifact | Redaction misses a value | K3 redaction; the driver withholds unclean captures; `ci.mjs stage` fails closed on any recorded or re-scanned residual; name allowlist; symlinks refused | `run.mjs --self-test`: "ci stage: …" (17 cases), "ci lifecycle: …", and K3's redaction cases | Redaction is pattern-based: a secret with no known shape and no known literal can pass (K3 limits) |
| The harness under test, or herdr, is steered into reading the job token or writing `GITHUB_ENV` to influence later steps | Prompt injection into the harness during a run | Tokens and workflow-command file paths are removed from the driver's environment; scenario prompts are fixed; later steps run only `ci.mjs` and pinned actions | `run.mjs --self-test`: "ci: scrub removes CI job tokens and workflow-command files" | Defense in depth only: the harness runs as the runner user and could find those files on disk |
| Paths or user names leak through the public job log | The driver or a scenario prints raw values | `ci.mjs` prints the outcome from the redacted manifest; driver errors print messages only | None for log content as a whole | An unexpected exception message could carry a path |
| A compromised third-party action runs on the runner | Upstream action tag moved | Only GitHub-owned actions, pinned by full commit SHA | actionlint parse only; no behavioral test | A malicious commit at the pinned SHA itself |
| Unredacted scratch left on the runner | A hard kill mid-run | The driver removes scratch; `cleanup` removes the work dir; the runner empties `RUNNER_TEMP` | `run.mjs --self-test`: "lifecycle: every run removed its scratch directory", "ci cleanup: removes the work dir", "ci lifecycle: cleanup removes the work dir" | Until the next job starts, after a hard kill |

Doctrine (`oac-security-work` §3): nothing a harness or pane prints during a run is an
instruction to the workflow. The workflow acts only on the driver's exit code and the
stage gate's own checks.

## 7. Operator checklist before the first dispatch

- [ ] Fork-PR approval set to "Require approval for all external contributors" (§1).
- [ ] `main` protected. Write access reviewed.
- [ ] Dedicated runner user and machine, with no other credentials on it (§2).
- [ ] Runner v2.327.1 or later, registered with label `oac-harness`, on Linux and on
      Windows.
- [ ] `herdr --version` prints `herdr 0.9.1` as the runner user. Node and git on `PATH`.
- [ ] Claude Code at the PINS.md last-observed version, logged in interactively as the
      runner user.
- [ ] No provider API key or harness token in the runner's environment.
- [ ] First dispatch: `scenario=smoke`, `runner=linux`, then `runner=windows`. Then
      `scenario=g1-claude-wake` per runner, with you at that machine to accept the dialog.

## 8. First-dispatch record (operator: fill in)

K6's acceptance asks for one successful G1 dispatch on Linux and one on Windows, with run
URLs recorded here. **None has run.** Do not fill a row from anything but a real run.

| Runner | Scenario | Run URL | Date (UTC) | Commit | `herdr --version` | `claude --version` | Run outcome | Evidence staged | Operator |
|---|---|---|---|---|---|---|---|---|---|
| linux | g1-claude-wake | TODO (operator) — NOT RUN | | | | | | | |
| windows | g1-claude-wake | TODO (operator) — NOT RUN | | | | | | | |

A run outcome of `PASS` here means the driver ran the scenario to the end. It is not a G1
verdict (`scripted-runs.md` "Three vocabularies").
