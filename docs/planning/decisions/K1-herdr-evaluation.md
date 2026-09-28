# K1: herdr evaluation — go/no-go and exact pin

**Issue:** #124 (Epic K #123, backlog key `K1`). **Depends on:** none. **Source:**
issue #123 Phase 1; `docs/planning/backlog/06-tasks-K.json` key `K1`.

**Status:** **Provisional go — pending live confirmation.** This is not a final verdict.
The desk leg (first-party docs and source at the pinned tag) is complete and found no
disqualifier. The live leg — every acceptance item that needs herdr driving a real,
logged-in Claude Code or Codex session — is **NOT RUN**. The go/no-go becomes final only
when the live leg in §6 has run and been recorded in this file. Issue #124 stays open
until then.

**Why the live leg is NOT RUN.** This record was written by a cloud agent session with no
herdr binary, no Claude Code install and no Codex install, and no harness login. Per
`[ADR-001 Boundary]` "MUST NOT steal or reuse another harness's provider credentials" and
issue #123's own execution-scope note, that session must never hold harness
credentials. So it could not run the live leg. It did not guess at live results. This is
the same `NOT RUN` stance `docs/planning/gates/G3-result.md` takes for its parked macOS
leg: a leg that could not be executed is recorded as not run, with the blocker named. It
is never scored as a pass because the documentation "should" hold.

**Where this record lives.** `docs/planning/decisions/` holds the C-series decisions
(`C1-language-runtime.md` through `C7-zenoh-transport.md`). K1 is a test-tooling decision,
not a v0.1 product decision. It is **not** folded into
`docs/planning/v0.1/03-decisions-and-amendments.md`. No ADR-001 text is changed by it.
`docs/planning/STATUS.md` carries a pointer to this file ("Decisions landed", "Open
UNVERIFIED items").

**Scope guard, stated once.** herdr is dev/test tooling only. Nothing in this file adds
herdr to `adapters/`, `core/`, `cli/`, `transports/` or `spec/`, or to the shipped
dependency inventory (`docs/planning/v0.1/07-repository-and-dependencies.md` §5 records it
under a separate not-shipped heading). No spike code or scripts are committed by K1.

---

## 0. Timebox

- **Desk leg:** run 2026-09-28 by a cloud agent session. No timebox was declared before
  desk research started. That is a process deviation from `oac-gates`' timebox policy,
  recorded here as `docs/planning/gates/G1-result.md` records the same deviation for its
  original run. The desk record was written as-is at 2026-09-28T15:00Z (approximately).
  It is not extended past that point.
- **Live leg:** **NOT RUN.** The operator declares its timebox **before the first
  command** in §6 and writes the record as-is when the box expires. Proposed box: 60
  minutes per OS. That matches G4's re-run and G5's boxes (`docs/planning/STATUS.md`
  "Gate verdicts"). An expired box with items still unconfirmed leaves those items
  `UNVERIFIED`; it does not license a guess.

The K1 acceptance item "Timebox declared before starting; the record is written as-is
when it expires" is therefore **not met** for the desk leg, and it is **open** for the
live leg (§10).

## 1. Decision

1. **Provisional go.** herdr is adopted as Epic K's test-side driver for real Claude Code
   and Codex CLI sessions. This holds only until the live leg (§6) runs. Nothing found at
   the desk leg disqualifies it (§5). Every behavior the driver depends on is either
   documented in herdr's own first-party docs at the pinned tag, or confirmed in its
   source at that tag. None of it has been observed live.
2. **Final go/no-go: NOT RUN.** This is decided by the live leg against the no-go
   conditions in §8.
3. **Exact pin: herdr `v0.9.1`,** a fixed pin, not floating (§2). The license is
   **Apache-2.0** (§3).
4. **Two desk findings K3 must design around.** Neither one is disqualifying.
   - (a) `agent read` and `agent send-keys` take **no `--timeout` option**. On
     `agent prompt`, `--timeout` is accepted only together with `--wait`. The acceptance
     item "each take an explicit timeout" is therefore **refuted at the desk** for two of
     the five commands (§5, item 3). The K3 driver must bound those two calls with its
     own process-level deadline.
   - (b) **No first-party herdr source documents that Codex goes to `unknown` after a
     response.** The premise in the acceptance item was not found at the pinned tag
     (§5, item 5). The live leg must observe which state Codex actually settles in. K7's
     bounded `unknown` handling must be built on that observation, not on the premise.

## 2. Pin

- **Version and release tag:** `v0.9.1`. The annotated tag object is
  `8544776216a8d28088db59a5344ea21ee2d05d2b` and points to commit
  `065ef9d6a531c49fb8bee7e818ef837065b21ee9` ("release: v0.9.1"). The tagger timestamp is
  2026-09-16T18:30:33Z. The tag message reads `Previous-Stable: v0.9.0`. Source: `git
  ls-remote --tags https://github.com/herdrdev/herdr` and a clone of tag `v0.9.1`,
  retrieved 2026-09-28.
- **Release page:** https://github.com/herdrdev/herdr/releases/tag/v0.9.1, labelled
  **Latest** on https://github.com/herdrdev/herdr/releases and published Sep 16, retrieved
  2026-09-28. `CHANGELOG.md` at the tag reads `## [0.9.1] - 2026-09-16`, and
  `Cargo.toml` at the tag reads `version = "0.9.1"`. Source:
  https://github.com/herdrdev/herdr/blob/v0.9.1/CHANGELOG.md and
  https://github.com/herdrdev/herdr/blob/v0.9.1/Cargo.toml, tag `v0.9.1`, retrieved
  2026-09-28.
- **Still current.** Issue #123's research line gave `v0.9.1` "as of 2026-09-16". That
  was re-checked rather than trusted. As of 2026-09-28, the newest stable tag is still
  `v0.9.1`, with no `v0.9.2` or `v0.10.x`. The only newer releases are the **preview**
  prereleases `preview-2026-09-21-0ff0f27e2226` and `preview-2026-09-28-80c0c07250d2`,
  both marked "Pre-release". Source: https://github.com/herdrdev/herdr/releases and
  `git ls-remote --tags`, retrieved 2026-09-28. Preview builds "can regress", in
  herdr's own words (`docs/next/website/src/content/docs/install.mdx` L138 at tag
  `v0.9.1`), so they are not pin candidates.
- **Why a fixed pin works here, unlike the floating Claude Code and Codex rows.** herdr
  does not install its own updates in the background. It "checks for new releases and
  notifies you in the app", and `herdr update` is a manual command
  (`install.mdx` L116-120 at tag `v0.9.1`, retrieved 2026-09-28). The background version
  check itself can be switched off with `update.version_check = false`
  (`docs/next/website/src/data/config-reference.json` L461 at tag `v0.9.1`). The fixed pin
  can therefore be enforced by the driver refusing any other `herdr --version`, which
  K3's acceptance already requires.
- **Caveat: the pin does not fix detection rules.** "Herdr also checks herdr.dev for
  remote manifest updates and applies valid per-agent rule updates automatically without
  requiring a Herdr restart. … Set `[update] manifest_check = false` to disable background
  remote manifest checks." Source:
  https://github.com/herdrdev/herdr/blob/v0.9.1/docs/next/website/src/content/docs/agents.mdx
  L67, tag `v0.9.1`, retrieved 2026-09-28. With the binary pinned but this left on, the
  `idle`/`working`/`blocked`/`unknown` classification the driver schedules on can change
  underneath the pin. K3 input: the driver runs herdr with its own config file
  (`HERDR_CONFIG_PATH`, `cli-reference.mdx` L535) that sets `update.manifest_check =
  false` and `update.version_check = false`. It records the active manifest versions in
  the run manifest (`herdr server agent-manifests --json`, `cli-reference.mdx` L126). The
  bundled manifests at the tag are `claude` `2026.09.11.1` and `codex` `2026.09.14.1`
  (`src/detect/manifests/claude.toml` L2, `src/detect/manifests/codex.toml` L2).
- **Release integrity (informational).** The release carries a `Release attestation
  (json)` asset alongside the binaries (§4). Verifying it is a K3 option. K1 does not
  require it.

## 3. License

**Apache-2.0.** Three independent first-party statements at the pinned tag agree:

- `LICENSE` is the Apache License, Version 2.0 text. Source:
  https://github.com/herdrdev/herdr/blob/v0.9.1/LICENSE, tag `v0.9.1`, retrieved
  2026-09-28.
- `Cargo.toml` `[package]` has `license = "Apache-2.0"` (L7). Source:
  https://github.com/herdrdev/herdr/blob/v0.9.1/Cargo.toml, tag `v0.9.1`, retrieved
  2026-09-28.
- The `README.md` "license" section says "Herdr is licensed under the [Apache License
  2.0](LICENSE)." Source: https://github.com/herdrdev/herdr/blob/v0.9.1/README.md, tag
  `v0.9.1`, retrieved 2026-09-28.

herdr is not linked into, vendored into, or shipped with the `oac` binary. It is invoked
as an external process by test tooling only. It therefore adds nothing to the Apache-2.0
compatibility verdict for the shipped inventory
(`docs/planning/v0.1/07-repository-and-dependencies.md` §7). It is recorded under §5's
not-shipped heading in that file. The Stage 6 license inventory (`oac-release`) should
list it as a dev/test tool, not as a distribution dependency.

## 4. OS support — a documentation claim, not live-tested

Every cell in this table is **herdr's own documentation claim** at tag `v0.9.1`. None of
it was observed in this record.

| OS | herdr's documented claim at `v0.9.1` | Release asset at `v0.9.1` | Live-tested here |
|---|---|---|---|
| Linux | Stable-channel binaries for x86_64 and aarch64 | `herdr-linux-x86_64`, `herdr-linux-aarch64` | **No** — UNVERIFIED, pending operator run |
| macOS | Stable-channel binaries for Intel and Apple silicon | `herdr-macos-x86_64`, `herdr-macos-aarch64` | **No** — UNVERIFIED, pending operator run |
| Windows | "Native Windows support is generally available", with "documented platform-specific limitations"; x86_64 binary; "Windows ARM64 runs the x86_64 build under Windows emulation" | `herdr-windows-x86_64.zip` (contains `herdr.exe` and "its app-local ConPTY runtime") | **No** — UNVERIFIED, pending operator run |

Sources, all at tag `v0.9.1` and retrieved 2026-09-28:

- `docs/next/website/src/content/docs/install.mdx` L6 ("Herdr publishes stable-channel
  binaries for Linux, macOS, and Windows. Windows is generally available, with documented
  platform-specific limitations and ongoing fixes."), L83-91 (asset table), L100-102
  (Windows archive), and L152-154 (Requirements).
- `docs/next/website/src/content/docs/windows-beta.mdx` L6 and its "Supported on Windows"
  table, L28-48. That table lists "Local persistent sessions", "Pane launch cwd", "Agent
  command discovery" and "Agent process-tree detection" as `supported`, and "Live cwd
  after shell `cd`" as `partial`.
- Release asset list: https://github.com/herdrdev/herdr/releases/expanded_assets/v0.9.1:
  `herdr-linux-aarch64`, `herdr-linux-x86_64`, `herdr-macos-aarch64`,
  `herdr-macos-x86_64`, `herdr-windows-x86_64.zip`, source archives, and `Release
  attestation (json)`.

Windows notes relevant to Epic K, all documented and not observed:

- The v0.9.1 changelog records "Windows Codex prompts reliably submit after pasted text
  instead of losing Enter" (#3187, #3961).
- `agent prompt` sends Codex on Windows "a paste boundary before Enter"
  (`cli-reference.mdx` L352).
- The changelog also says of #3748 that the Git Bash detach report "is still under
  investigation; do not treat this release as a confirmed fix". The Windows live leg
  should record which pane shell it used (PowerShell, `cmd.exe` or Git Bash).

Per-OS support becomes a verified fact only when §6 has run on that OS.

## 5. Desk findings, one per acceptance item

Each item is confirmed or refuted individually, as K1's acceptance requires. "Desk
status" is what the first-party docs or source at tag `v0.9.1` state. "Live status" is
what has been observed. Throughout, "docs" means
`https://github.com/herdrdev/herdr/blob/v0.9.1/docs/next/website/src/content/docs/<file>`
and "source" means `https://github.com/herdrdev/herdr/blob/v0.9.1/<path>`, both at tag
`v0.9.1` (commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`) and retrieved 2026-09-28.
The docs tree cited is `docs/next` **at the release tag**. herdr's `docs/versions/README.md`
says release CI "creates each version from the tagged `docs/next` tree", so this is the
v0.9.1 documentation.

### Item 1 — Named-session start with no attached terminal

- **Desk status: documented.**
  - "Add `--session <name>` to use a named session instead of the default."
    (`cli-reference.mdx` L26.)
  - "`herdr server` runs the headless server explicitly. Use it for supervised or
    service-style setups." (L131.)
  - `HERDR_SESSION`: "Select a named session for CLI commands." (L536.)
  - "A named session has its own panes, tabs, workspaces, sockets, and runtime state. It
    still shares the same global config file." (`persistence-remote.mdx` L28.)
  - CLI commands do not auto-start a server. With none running they return
    `server_not_running` (source `src/cli/server_not_running.rs` L19-31). The server must
    therefore be started explicitly with `herdr --session <name> server`. `--session` is
    a known top-level flag (source `src/main.rs` L731-742).
- **Live status: UNVERIFIED — pending operator run** (§6, step L1).

### Item 2 — `agent start --kind claude` / `--kind codex` accept arbitrary argv and cwd

- **Desk status: documented, with two qualifications.**
  - Usage is `herdr agent start <name> --kind KIND --pane ID [--timeout MS] [--
    <agent-args...>]` (`cli-reference.mdx` L341). "`--kind` selects a supported agent and
    its canonical executable. … Arguments after `--` are passed unchanged to that
    executable." (`agent-automation.mdx` L44.)
  - The canonical executables are `claude` and `codex` (source `src/detect/mod.rs`
    L997-1001).
  - A successful start returns the launched `argv` in its result (source
    `src/api/schema/response.rs` L100-103). The driver can record the verbatim launch
    argv from that value.
  - **Qualification 1 — the argv is not fully arbitrary.** argv[0] is fixed by `--kind`
    (`claude` or `codex`). Only the arguments after `--` are caller-controlled. A wrapper
    command such as `env X=1 claude …` cannot be expressed through `agent start`.
    Environment goes on the pane instead: `workspace create`, `tab create` and
    `pane split` each take `--env KEY=VALUE` (`cli-reference.mdx` L156, L191, L217).
  - **Qualification 2 — cwd is not an `agent start` option.** It is set when the pane
    is created, with `--cwd PATH` on `workspace create`, `tab create` or `pane split`
    (same lines). `agent start` then requires "an existing available shell pane"
    (`agent-automation.mdx` L42). herdr reports `foreground_cwd` on `agent get` when it
    can resolve it (`cli-reference.mdx` L298).
  - The G1 launch command `claude --dangerously-load-development-channels
    server:<name>` (`docs/planning/gates/G1-result.md`) therefore maps to `herdr agent
    start <name> --kind claude --pane <pane> -- --dangerously-load-development-channels
    server:<name>`.
- **Live status: UNVERIFIED — pending operator run** (§6, steps L2 and L4).

### Item 3 — Explicit timeouts on `agent prompt --wait`, `agent wait --until`, `agent read`, `pane wait-output`, `agent send-keys`

| Command | `--timeout MS` at `v0.9.1`? | Evidence | Desk status |
|---|---|---|---|
| `agent prompt --wait` | Yes, but only with `--wait` (`.requires("wait")`) | `cli-reference.mdx` L336, L352; source `src/cli/spec.rs` L339-362 | confirmed |
| `agent wait --until` | Yes. "Without --timeout, waits indefinitely." | `cli-reference.mdx` L339, L403; source `src/cli/spec.rs` L378-391 | confirmed |
| `pane wait-output` | Yes. It waits "indefinitely when `--timeout` is omitted" | `cli-reference.mdx` L394, L403 | confirmed |
| `agent read` | **No.** The options are `--source`, `--lines`, `--format`, `--ansi` | `cli-reference.mdx` L334; source `src/cli/spec.rs` L322-331 | **refuted** |
| `agent send-keys` | **No.** The options are `<target> <key>...` | `cli-reference.mdx` L335; source `src/cli/spec.rs` L332-338 | **refuted** |

- **Consequence for K3.** Every wait the driver issues must pass `--timeout`, because
  herdr's own default is to wait indefinitely (`agent-automation.mdx` L96). `agent read`
  and `agent send-keys` are one-shot socket requests, not waits. The driver bounds them
  with a process-level deadline on the `herdr` child process. The refutation stands on
  first-party source.
- **Timeout outcomes.** A timeout or server error "print[s] a JSON error to stderr and
  exit[s] with status 1; invalid CLI syntax exits with status 2" (`agent-automation.mdx`
  L96). "A timeout or `agent_prompt_stalled` does not prove that no input was sent. Read
  the agent before retrying" (L80). This matches K3's rule that a timeout ends the run as
  `NOT RUN` and never re-submits.
- **Live status:** the three confirmations and two refutations are **UNVERIFIED —
  pending operator run** (§6, step L6). The live check only confirms that the pinned
  binary behaves as its source says.

### Item 4 — Dialog text readable through `agent read` before any keystroke

- **Desk status: a source-derived expectation, not a documented guarantee.**
  - "If detection reports `blocked` during startup, the command returns `agent_not_ready`
    immediately. The name remains available for `agent read` and `agent send-keys`"
    (`agent-automation.mdx` L46).
  - Claude's bundled manifest rule `live_blocked_form` (`state = "blocked"`) matches
    `"esc to cancel"` together with `"enter to confirm"`, inside the region
    `after_last_horizontal_rule` (source `src/detect/manifests/claude.toml` L78-94).
  - The dev-channels dialog captured verbatim in G1 Box C ends `Enter to confirm · Esc to
    cancel` (`docs/planning/gates/G1-result.md`, criterion 5).
  - The expected shape is therefore: `agent start … -- --dangerously-load-development-channels
    server:<name>` returns `agent_not_ready`, `agent read` then shows the dialog text, and
    only after that does `agent send-keys <name> enter` accept it.
  - **Why this is only an expectation.** Whether the rule's region actually matches the
    dialog's layout is not established. If it does not match, herdr falls back to `idle`
    ("If no manifest rule matches for a known agent, Herdr falls back to `idle`",
    `agents.mdx` L61). `agent start` would then report success while the dialog is still
    on screen, and a later `agent prompt` would type into the dialog.
  - The driver must never trust herdr's state here. It reads the pane with
    `--source visible` and checks for the dialog text before sending any input.
- **Live status: UNVERIFIED — pending operator run** (§6, step L3). This item is
  load-bearing for G1 criterion 5 parity in K4. If it fails, see §8.

### Item 5 — Codex's documented `unknown` state after a response

- **Desk status: premise not found — UNVERIFIED.** No first-party herdr source at
  `v0.9.1` states that Codex settles to `unknown` after a response. What the sources do
  say:
  - `unknown` in general: "`unknown` means an agent is present but Herdr cannot classify
    its lifecycle confidently; it does not prove successful completion"
    (`agent-automation.mdx` L78).
  - `agent wait` and `agent prompt --wait` "default to `idle`, `done`, or `blocked`; use
    `--until unknown` explicitly when needed" (`cli-reference.mdx` L352).
  - Codex's state is "screen manifest" (`agents.mdx` L30).
  - The bundled Codex manifest (`2026.09.14.1`) has exactly one `unknown` rule,
    `transcript_viewer`. It has `skip_state_update = true` and matches the transcript
    viewer's scroll hints (source `src/detect/manifests/codex.toml` L22-32). That is a
    viewer state, not a post-response state.
  - The v0.9.1 changelog says "Codex stays working with static titles, animations
    disabled, and queued follow-ups. Composer sparkles and old confirmation text no longer
    make an idle pane appear blocked." (#4092, #4099, #3988.) It says nothing about
    `unknown`.
- **Consequence.** A wait that uses only the default states could hang (until its
  timeout) if Codex does settle to `unknown`. The live leg observes the actual post-response
  state with every state requested, and records `agent explain --json` as the reason.
- **Live status: UNVERIFIED — pending operator run** (§6, step L5).

### Item 6 — Launch-environment delta; harness config untouched; `integration install` never run

- **Desk status: source-derived, not observed.**
  - Every pane process gets `HERDR_ENV=1` (source `src/pane.rs` L156, `src/main.rs`
    L3-4) and `HERDR_SOCKET_PATH` and `HERDR_BIN_PATH` (source
    `src/integration/env.rs` L28-33).
  - Managed panes also get `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID` and `HERDR_PANE_ID`
    (source `src/pane.rs` L159-169).
  - On Windows, a Git Bash pane also gets `HERDR_PANE_RUNTIME_ID` (source
    `src/platform/windows.rs` L373, L880-884).
  - Non-`HERDR_*` changes: `TERM=xterm-256color` and `COLORTERM=truecolor` are set, and
    `WT_SESSION` is removed (source `src/pane.rs` L68-69, L92-100). `CODEX_THREAD_ID` and
    `OMPCODE` are also removed (source `src/pane.rs` L147-152).
  - Caller `--env` values are added, but "Herdr-managed variables … stay authoritative
    when they conflict with caller-provided env" (`cli-reference.mdx` L518).
  - The documented variable list is `cli-reference.mdx` L531-545.
  - **The removal of `CODEX_THREAD_ID` is a real delta for the Codex leg.** Whether it
    matters for any gate launch is for the live leg to record, not assume.
- **Desk status of `integration install`: documented, and never run.**
  - `integration install claude` "writes `hooks/herdr-agent-state.sh` and updates
    `settings.json` with Herdr hook entries" (`integrations.mdx` L146).
  - `integration install codex` "writes `herdr-agent-state.sh`, updates `hooks.json`,
    and ensures `[features] hooks = true` in `config.toml`" (L158). Its uninstall "leaves
    `config.toml` unchanged" (L158), so installing it would leave a permanent config
    change behind.
  - For both Claude Code and Codex, the integration role is "session" only, and state
    authority is "screen manifest" whether or not it is installed (`agents.mdx` L29-30).
  - Installing it therefore gains the driver nothing and costs a harness-config
    mutation. It is **never run**. K2's lint enforces this in `tools/herdr/`.
- **Live status: UNVERIFIED — pending operator run** (§6, steps L0, L7, L8). This covers
  the actual env delta per OS, the before/after hashes of `settings.json`, `hooks.json`
  and `config.toml`, and the shell history showing no `integration install`.

## 6. Live-verification checklist (operator-run, NOT RUN)

Every step below is **UNVERIFIED — pending operator run**. Run it on each OS where Epic K
will drive harnesses, at minimum Linux and Windows per Epic K success criterion 5. Run it
on a machine where Claude Code and Codex are already logged in, with the operator at the
keyboard. **Declare the timebox before L0** and write the results into §7 as-is when it
expires.

The commands are bash (Linux, macOS, or WSL). On native Windows the `herdr` commands are
identical. Adapt only the shell plumbing: `$env:VAR`, `Get-FileHash` for `sha256sum`,
`Get-ChildItem Env:` for `env`, and `ConvertFrom-Json` for `jq`. Do not commit raw pane
or env captures. They can contain tokens, usernames and paths. Record only the fields
named under each step's "Evidence to record".

**L0 — Preflight and baselines**

```bash
herdr --version                        # must print 0.9.1; stop otherwise
claude --version; codex --version      # record both (floating pins, PINS.md)
export K1_SCRATCH="$(mktemp -d)"       # outside the repo
cd "$K1_SCRATCH"; mkdir claude codex shell
printf '[update]\nversion_check = false\nmanifest_check = false\n' > herdr-config.toml
export HERDR_CONFIG_PATH="$K1_SCRATCH/herdr-config.toml"
env | sort > host-env.txt              # parent env of the herdr server
CL="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"; CX="${CODEX_HOME:-$HOME/.codex}"
sha256sum "$CL/settings.json" "$CX/hooks.json" "$CX/config.toml" 2>&1 | tee hashes-before.txt
```

Evidence to record: the three version strings; `hashes-before.txt` (hashes only, paths
redacted); the timebox start time.

**L1 — Named session, no attached terminal (item 1)**

```bash
herdr --session k1eval server > server.log 2>&1 &   # headless; no client ever attached
export HERDR_SESSION=k1eval
herdr status server
herdr session list --json
herdr server agent-manifests --json | tee manifests.json
```

Expected shape: `status server` reports a running server for `k1eval`, and
`session list` shows `k1eval`. No `herdr` / `herdr session attach` client is started at
any point in L1-L9. Evidence to record: both outputs, verbatim (redacted), and the
manifest versions.

**L2 — `agent start --kind claude` with the G1 argv and a chosen cwd (item 2)**

Prerequisite: `$K1_SCRATCH/claude/.mcp.json` registers some stdio MCP server under the
name `k1probe`. For example, point it at a copy of the G1 channel server kept **outside
the repo**: `docs/planning/gates/fixtures/g1-claude-wake/channel-server.mjs.throwaway-quarantined`.
K1 needs only the startup dialog, not channel delivery.

```bash
ws=$(herdr workspace create --cwd "$K1_SCRATCH/claude" --label k1-claude \
       --env MCP_PROTOCOL_NEGOTIATION=legacy --no-focus)
cpane=$(printf '%s\n' "$ws" | jq -r '.result.root_pane.pane_id')
herdr agent start k1claude --kind claude --pane "$cpane" --timeout 60000 \
  -- --dangerously-load-development-channels server:k1probe; echo "exit=$?"
herdr agent get k1claude
```

Expected shape: either a success result that carries `argv` (it should read `claude
--dangerously-load-development-channels server:k1probe`), or an `agent_not_ready` JSON
error on stderr (see item 4). `agent get` shows `foreground_cwd` equal to
`$K1_SCRATCH/claude` when herdr can resolve it. Evidence to record: the exit code, the
result or error JSON verbatim, and `foreground_cwd` (redacted to `<scratch>/claude`).

**L3 — Dialog readable before any keystroke (item 4)**

No input may reach the pane before the read below is saved.

```bash
herdr agent read k1claude --source visible | tee claude-dialog.txt
herdr agent explain k1claude --json | tee claude-explain.json
# only now:
herdr agent send-keys k1claude enter     # option 1 is preselected in G1's capture
herdr agent wait k1claude --until idle --until done --timeout 60000; echo "exit=$?"
```

Expected shape: `claude-dialog.txt` contains the dialog text as G1 Box C captured it
(`WARNING: Loading development channels` … `Enter to confirm · Esc to cancel`). Also
record the state `explain` reported. If L2 returned success rather than `agent_not_ready`
while this dialog was on screen, that is a finding. It means herdr's state would have let
a prompt type into the dialog; record it under §8. Evidence to record: the dialog text
verbatim, the explain state and rule id, and the wait exit code.

**L4 — `agent start --kind codex` (item 2)**

```bash
ws=$(herdr workspace create --cwd "$K1_SCRATCH/codex" --label k1-codex --no-focus)
xpane=$(printf '%s\n' "$ws" | jq -r '.result.root_pane.pane_id')
herdr agent start k1codex --kind codex --pane "$xpane" --timeout 60000; echo "exit=$?"
herdr agent read k1codex --source visible | tee codex-start.txt
herdr agent get k1codex
```

The argument list is left empty on purpose. G2's primary path launches a plain TUI (see
`docs/planning/gates/G2-result.md`), and this repo cites no Codex flag that is both
harmless here and first-party-verified, so none is invented. If the operator runs G2's
fallback path, pass its `--remote ws://…` URL after `--` and record it. Expected shape:
success with `argv` equal to `codex`, or `agent_not_ready` if a startup prompt is up
(the manifest has `trust_directory` and `startup_update` blocked rules,
`src/detect/manifests/codex.toml` L34-53). Evidence to record: the exit code, the
result/error JSON, and the visible text if blocked.

**L5 — Codex state after a response (item 5)**

This sends one real prompt, which uses the operator's own Codex account.

```bash
herdr agent prompt k1codex "Reply with the single word ok." --wait \
  --until idle --until done --until blocked --until unknown --timeout 120000 | tee codex-prompt.json
herdr agent get k1codex | tee codex-after.json
herdr agent explain k1codex --json | tee codex-explain.json
sleep 10; herdr agent get k1codex          # second sample: does the state move on its own?
```

Evidence to record: the state in `codex-prompt.json` / `codex-after.json`, the explain
rule id, and the second sample. Only this observation can confirm or refute the
"`unknown` after a response" premise.

**L6 — Timeouts (item 3)**

```bash
herdr agent wait k1codex --until working --timeout 5000; echo "exit=$?"            # expect exit 1, JSON timeout on stderr
herdr pane wait-output "$xpane" --match 'K1-NEVER-MATCHES' --timeout 5000; echo "exit=$?"   # expect exit 1
herdr agent prompt k1claude "Reply with the single word ok." --wait --timeout 1; echo "exit=$?"  # expect timeout or agent_prompt_stalled; then read before any retry
herdr agent read --help; herdr agent send-keys --help                              # expect no --timeout option listed
```

`agent read`/`send-keys` are checked through `--help`, never by sending a key with a
guessed flag. Evidence to record: each exit code and stderr JSON `code`, and the two help
outputs' option lists.

**L7 — Launch-environment delta (item 6)**

```bash
ws=$(herdr workspace create --cwd "$K1_SCRATCH/shell" --label k1-shell --no-focus)
spane=$(printf '%s\n' "$ws" | jq -r '.result.root_pane.pane_id')
herdr pane run "$spane" "env | sort > '$K1_SCRATCH/pane-env.txt'; echo K1-ENV-\$((1+1))"
herdr pane wait-output "$spane" --match 'K1-ENV-2' --timeout 10000   # sentinel only appears in output, not in the echoed command line
comm -13 host-env.txt pane-env.txt | cut -d= -f1 > added-or-changed.txt
comm -23 host-env.txt pane-env.txt | cut -d= -f1 > removed-or-changed.txt
```

Evidence to record: **variable names only** from both files, never values except for
`HERDR_ENV`, `TERM` and `COLORTERM`. Compare them with item 6's source-derived list. Any
name that is not on that list is a finding. Shell rc files can add names too, so note
the shell used.

**L8 — Harness config unchanged; `integration install` never run (item 6)**

```bash
sha256sum "$CL/settings.json" "$CX/hooks.json" "$CX/config.toml" 2>&1 | tee hashes-after.txt
diff hashes-before.txt hashes-after.txt && echo UNCHANGED
history | grep -c 'herdr integration install'     # expect 0 (or the platform equivalent)
```

Evidence to record: `UNCHANGED` or the diff, and the count. A changed hash is attributed
before it is blamed on herdr: the harness itself may write its own settings. If it
changed, repeat L2/L4 without herdr as a control.

**L9 — Teardown**

```bash
herdr session stop k1eval --json
herdr session delete k1eval --json
pgrep -fa herdr || echo "no herdr process"
```

Evidence to record: both outputs, and that no `herdr` process remains.

## 7. Live results (to be filled by the operator run)

| Item | Linux | macOS | Windows |
|---|---|---|---|
| 1 Named session, no attached terminal | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| 2 `agent start` argv/cwd (claude, codex) | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| 3 Timeouts (3 confirmed, 2 refuted at desk) | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| 4 Dialog readable before keystroke | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| 5 Codex state after a response | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| 6 Env delta; harness config unchanged; no `integration install` | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run | UNVERIFIED — pending operator run |
| Timebox (declared / elapsed) | NOT RUN | NOT RUN | NOT RUN |

When the live leg runs, it records the harness versions actually observed
(`claude --version`, `codex --version`). Both rows float in `docs/planning/PINS.md`. The
same change updates `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md` row 51, and changes this file's **Status** line to a
final go or no-go.

## 8. No-go and reversal conditions

The final verdict is **no-go** for the affected OS if the live leg shows any of these.
On Linux or Windows that means no-go for Epic K as scoped, since success criterion 5 needs
both.

- A named session cannot run headless with no client attached (item 1).
- `agent start --kind claude` cannot pass `--dangerously-load-development-channels
  server:<name>` through unchanged (item 2).
- The dev-channels dialog text cannot be read before the first keystroke (item 4). A
  driver that must press keys blind cannot give G1 criterion 5 evidence at parity with
  the human-run Box C.
- herdr changes a harness's `settings.json`, `hooks.json` or `config.toml` without
  `integration install` (item 6).
- A future re-pin shows a license other than Apache-2.0.

These are **not** no-go conditions. Each has a driver-side answer:

- The missing `--timeout` on `agent read`/`send-keys` (item 3). The driver applies a
  process-level deadline.
- Codex settling to `unknown`, `idle` or `done` after a response (item 5). The driver
  requests exact states explicitly and never scores on herdr state.
- A herdr state that misclassifies a dialog, as described under item 4. The driver
  always reads the visible text before sending input.

Reversal after a final go: any of the no-go conditions above appearing at a later herdr
pin. A move of the `herdr (test tooling)` row in `docs/planning/PINS.md` re-opens this
record's live leg for the new version.

## 9. Surface labels, boundary pass, and UNVERIFIED ledger

**Labels** (`oac-evidence` §4):

| Surface | Label | Note |
|---|---|---|
| herdr CLI / socket API (`agent`, `pane`, `session`, `server` commands) | supported | Documented in herdr's own first-party CLI reference at `v0.9.1`. herdr is test tooling, not an OAC provider surface |
| herdr agent-state classification (`idle`/`working`/`blocked`/`done`/`unknown`) | supported (scheduling signal only) | Documented. It is derived from screen manifests for Claude Code and Codex (`agents.mdx` L29-30) and **never used as OAC evidence** (issue #123 non-goals) |
| herdr live handoff (`--handoff`) | experimental | Stated as experimental by herdr (`install.mdx` L144-147). Not used by Epic K |
| `integration install claude` / `integration install codex` | supported | Documented. **Never run** by Epic K (item 6) |

No shim boundary is needed. herdr is not reached from product code at all. K2's
containment lint is the enforcement point.

**Boundary pass** (`oac-boundaries`):

- **Boundaries 4 and 13 (scraping and private RPCs).** herdr's Claude Code and Codex
  state detection is screen-derived (`agents.mdx` L29-30). That is acceptable only
  because herdr stays off the supported-integration path. It plays the operator's hands
  and eyes. The adapter under test still reaches Claude Code only through Channels, and
  Codex only through the app-server protocol. Enforcement lives in K2's lint and in the
  rule that no gate criterion is scored on herdr's state. If a scripted run could only
  work by having herdr inject input that a supported interface should have carried, that
  is a boundary-13 finding, not a script.
- **Boundary 3 (credentials).** herdr and the driver never read harness credentials. The
  harness in the pane uses its own login. Pane env captures (L7) record names only,
  because the operator's environment can contain secrets.
- **Harness configuration.** `integration install` is never run (item 6). K2's lint
  fails on it in `tools/herdr/`.
- **Polling.** `pane wait-output` polls a terminal snapshot (`agent-automation.mdx`
  L82). That is test-harness observation of a terminal, not an adapter polling an inbox.
  The no-polling assertion (`oac-testing` §4) is unaffected.
- **Network.** herdr contacts herdr.dev for version and manifest checks (§2). The driver
  config turns both off.

**New UNVERIFIED items** (ledger entries in `docs/planning/STATUS.md` "Open UNVERIFIED
items" and `docs/planning/v0.1/11-risks.md` row 51):

- Every live-leg item in §7 (items 1-6, on each OS) (UNVERIFIED — the live leg is NOT
  RUN; no herdr binary or logged-in harness was available to the session that wrote this
  record).
- herdr's per-OS support (Linux, macOS, Windows) (UNVERIFIED — a documentation claim at
  `v0.9.1` only, §4).
- The "Codex settles to `unknown` after a response" premise (UNVERIFIED — not found in
  any first-party herdr source at `v0.9.1`, §5 item 5).
- Whether `agent start --kind claude … -- --dangerously-load-development-channels
  server:<name>` returns `agent_not_ready` while the dialog is up, as the bundled
  manifest suggests (UNVERIFIED — inferred from `src/detect/manifests/claude.toml`
  L78-94, §5 item 4).
- The full launch-environment delta (UNVERIFIED — derived from source at `v0.9.1`, not
  observed, §5 item 6).

**Closed at the desk by first-party evidence** (not UNVERIFIED): the pin `v0.9.1`
(§2); the Apache-2.0 license (§3); the absence of a `--timeout` option on `agent read`
and `agent send-keys` in the `v0.9.1` CLI spec (§5 item 3, which the live leg re-checks
only against the installed binary).

## 10. Acceptance boxes, ticked against lines in this file

- [ ] Timebox declared before starting; the record is written as-is when it expires. The
      desk leg declared none (a deviation, §0). The live leg is open (§0, §6).
- [x] `docs/planning/decisions/K1-herdr-evaluation.md` records go/no-go, the exact herdr
      version and release tag, and support per OS, every fact cited to the tagged
      release. Ticked with an explicit limit: the go/no-go is **provisional** (§1), and
      per-OS support is herdr's documentation claim only (§4).
- [ ] Each item confirmed or refuted individually. The desk status of every item is
      recorded (§5), including two desk refutations (item 3) and one premise not found
      (item 5). Live confirmation is **not done** (§7).
- [ ] Launch-environment delta recorded. It is source-derived only (§5 item 6). The
      observed delta and harness-config hashes are pending (§6 L7-L8).
- [x] Row `herdr (test tooling)` added to the `docs/planning/PINS.md` pin table with
      `Gates affected: none`, and `**Last updated:**` bumped.
- [x] Apache-2.0 license recorded under a dev/test-tooling, not-shipped heading in
      `docs/planning/v0.1/07-repository-and-dependencies.md` §5.
- [x] No spike code committed. UNVERIFIED items are listed open (§9).

## 11. Cross-file updates in this change

- `docs/planning/PINS.md`: new pin-table row `herdr (test tooling)` (`v0.9.1`, `Gates
  affected: none`), a new pin record "herdr (test tooling)", and `**Last updated:**`
  bumped. A row was added, so the pin-move checklist's last box was checked. No
  `G<n>-result.md` names herdr in `Pin rows relied on`, and no gate verdict is
  invalidated, because no gate relies on this row.
- `docs/planning/v0.1/07-repository-and-dependencies.md` §5: a new heading, "Dev/test
  tooling — not shipped". It records herdr's Apache-2.0 license and states that herdr is
  not part of the §5 inventory or the §7 verdict. §9 gets a surface-label row.
- `docs/planning/STATUS.md`: a `**Last updated:**` entry, a "Decisions landed" pointer,
  and one "Open UNVERIFIED items" entry for this record's pending live leg.
- `docs/planning/v0.1/11-risks.md`: new risk `RISK-HERDR` (R5, not a v0.1 dependency)
  and traceability row 51.
- No skill file, backlog file, gate result or ADR text is changed.

## 12. Sources

All retrieved 2026-09-28, at herdr tag `v0.9.1`, commit
`065ef9d6a531c49fb8bee7e818ef837065b21ee9`, unless stated:

- https://github.com/herdrdev/herdr/releases (release list; `v0.9.1` "Latest")
- https://github.com/herdrdev/herdr/releases/tag/v0.9.1 and
  https://github.com/herdrdev/herdr/releases/expanded_assets/v0.9.1 (asset list)
- `git ls-remote --tags https://github.com/herdrdev/herdr` (tag and commit SHAs)
- https://github.com/herdrdev/herdr/blob/v0.9.1/LICENSE, `Cargo.toml`, `README.md`,
  `CHANGELOG.md`
- https://github.com/herdrdev/herdr/tree/v0.9.1/docs/next/website/src/content/docs:
  `install.mdx`, `windows-beta.mdx`, `cli-reference.mdx`, `agent-automation.mdx`,
  `agents.mdx`, `integrations.mdx`, `persistence-remote.mdx`
- https://github.com/herdrdev/herdr/blob/v0.9.1/docs/next/website/src/data/config-reference.json
- herdr source at the tag: `src/cli/spec.rs`, `src/cli/server_not_running.rs`,
  `src/main.rs`, `src/pane.rs`, `src/integration/env.rs`, `src/platform/windows.rs`,
  `src/detect/mod.rs`, `src/detect/manifests/claude.toml`,
  `src/detect/manifests/codex.toml`, `src/api/schema/response.rs`
