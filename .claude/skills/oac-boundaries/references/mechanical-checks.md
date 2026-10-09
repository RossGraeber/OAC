# Mechanical boundary checks

The boundary lint scripts are `scripts/check-herdr-containment.mjs` (checks 9 and 10) and
`scripts/check-containment.mjs` (checks 12 and 13, #61).
Checks 1-8 have no script: this file **is** the check — run the whole list as part of every
`type:code` / `type:spec` work item, not just once. (`scripts/check-skills.mjs` checks skill
budgets, not ADR-001 boundaries.) `.github/workflows/boundary-lint.yml` runs checks 1, 2, 3, 8, 9,
10, 11, 12 and 13 on every pull request and push to main. Checks 1-2 run over `spec/`, which is mandatory
(a missing `spec/` fails), and over `core/` once it exists, together with the zero-hits group
of `oac-spec-authoring` `references/neutral-vocabulary-check.md` over `spec/` (#41). Check 2
and that group exempt exactly `spec/bindings/mcp.md` (the task E6 binding document); check 1
exempts nothing. `--hidden` keeps dot-files in scope, and `--no-ignore` stops a committed
`.ignore`, `.rgignore` or `.gitignore` from switching a check off (checks 3 and 8 carry it
too). In CI the spec checks run on an explicit `find` list of every regular file under
`spec/` (and `core/`), so the exemption is that one regular file and nothing else: a
non-regular `spec/bindings/mcp.md`, or any symlink under `spec/`, fails the step. The
commands below are the manual equivalents. Checks 4-7 are not wired to CI and stay manual.

All commands are Git Bash / ripgrep syntax. `spec/` exists since 2026-10-03 (#41).
`core/`, `adapters/claude/`, `adapters/codex/`, `cli/` and `transports/zenoh/` exist since
the F1 scaffold (#50, 2026-10-05) as skeleton crates, so every check below now runs against
real paths; treat any match as a stop-and-cite event. (A path that still does not exist
makes ripgrep error "cannot find the file specified": that means **pending**, not passing.)

Status 2026-10-05 (#50, F1 scaffold): checks 1-8 and 11 return zero hits over the
scaffold, and `node scripts/check-herdr-containment.mjs` reports `Result: CLEAN` across all
10 targets (no target PENDING). The paragraph below is the earlier record.

Status as last verified against this repo (2026-09-28; checks 1, 2, 9 and 11 re-verified
2026-10-03, #41): checks 1 and 2 are **clean** on `spec/` (zero hits) and **pending** on
`core/`; checks 4, 5, 6, 7 are **pending** (target paths do not exist yet); checks 3 and 8 are **clean** (zero hits) against
the current tree, which is docs/backlog plus `scripts/` and `tools/herdr/`; check 10 is
**clean** on real content (`tools/herdr/`, K3's driver: every tracked entry, 0 hits), and
so are check 9's three workflow targets (K6: `boundary-lint.yml` and
`herdr-provider-optin.yml`; since 2026-10-02, #219, also `g3-macos-hosted.yml`; 0 hits),
while check 9's other targets (four of the five product paths, manifests outside
`tools/herdr/`, and `tests/integration/`) are still **pending** (`spec/` is clean since
#41), so the script's last line
reads `Result: PENDING`; its `--self-test` has 103 cases (one, a non-UTF-8 file name, is
skipped on file systems that reject it, e.g. Windows). Check 11 (added 2026-09-29)
is **clean** on `spec/` (since #41) and pending on its other product paths and root Cargo
manifests.

Checks 9 and 10 report pending themselves instead of via a ripgrep path error: a target with
no git-tracked files prints `PENDING`, the last line reads `Result: PENDING`, and the exit code
is 0 so CI stays green. `Result: PENDING` is still not a pass — only `Result: CLEAN` is. Any
violation exits 1 with `Result: FAIL`.

```bash
# 1. Zenoh vocabulary must not appear in the neutral spec or core types.
rg -n --hidden --no-ignore --glob '!target' -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness' spec/ core/

# 2. Provider-specific method names must not appear in neutral interfaces. The task E6
#    binding document, spec/bindings/mcp.md (that exact path only), is exempt.
rg -n --hidden --no-ignore --glob '!target' --glob '!spec/bindings/mcp.md' \
  'claude/channel|thread/queue/add|turn/steer|turn/start|thread/start|thread/resume|notifications/claude/channel' \
  spec/ core/

# 3. No direct provider SDK imports/deps anywhere in the code tree (adapters call harness
#    IPC, not model APIs). Excludes docs/ and backlog JSON, which legitimately discuss these
#    names; language is still an open §5 decision, so cover Rust, Python, TS/JS, Go, and
#    manifest files rather than assuming one toolchain.
rg -n --no-ignore --glob '!target' --glob '!docs/**' -i \
  '\bopenai\b|\banthropic\b|@anthropic-ai|from openai|import openai' \
  --glob '*.rs' --glob '*.py' --glob '*.ts' --glob '*.toml' \
  --glob '*.js' --glob '*.mjs' --glob '*.go' --glob '*.json' .

# 4. Removed/unsupported Codex surface must not be referenced from code (scoped away from
#    docs/ and backlog JSON, which correctly discuss the deprecation as history/citation —
#    unscoped, this check false-positives on PLANNING-PROMPT.md and the backlog itself).
rg -n --glob '!target' --glob '!docs/**' --glob '!*.json' \
  'codex mcp-server|codex-rs/mcp-server' adapters/ core/ cli/ transports/

# 5. Rollout-file manipulation instead of the app-server API.
rg -n --glob '!target' 'CODEX_HOME.*sessions|rollout.*\.jsonl' adapters/ core/

# 6. Polling loops inside an adapter (heuristic — read the hit, don't trust the grep alone;
#    a provider-supported streaming/event loop is fine, an application poll of an inbox
#    while claiming active-inbound support is not).
rg -n --glob '!target' -A3 'loop\s*\{|while\s+true|while true|sleep\(|poll' adapters/

# 7. Credential file/store access from an adapter (should be zero hits outside
#    auth-delegation code explicitly reviewed under oac-security-work).
rg -n --glob '!target' 'auth\.json|Keychain|CredentialManager|Secret Service' adapters/

# 8. The CLI/local-deployment path must not require a separately administered server.
#    zenohd (the Zenoh router) is allowed only as part of an optional non-local/LAN path,
#    never a required piece of normal local use.
rg -n --no-ignore -i --glob '!docs/**' 'dockerfile|docker-compose|kubernetes|helm|zenohd' .

# 9. herdr (Epic K dev/test tooling) must not reach product code: no `herdr` / `HERDR_`
#    (case-insensitive) in any git-tracked entry under adapters/ core/ cli/ transports/
#    spec/, and no workspace or package manifest outside tools/herdr/ referencing
#    tools/herdr. tests/integration/ (#146, the tools/herdr/README.md reuse contract) is
#    opt-in provider-integration tier and MAY name and spawn herdr -- deliberately not
#    under the herdr-token ban -- but stays a leaf: no product-path entry climbs to it
#    (`../tests/integration`) or symlinks into it. Manifests fail closed on anything that
#    could pull it into a default build: a root manifest naming tests/integration, a
#    tests/* or tests/** glob, or a bare `tests` member; a nested manifest climbing to
#    any of those; a manifest under tests/ (outside tests/integration/) naming
#    `integration` or a quoted glob. A future opt-in root target needs a deliberate,
#    self-tested exception here. Nothing under it imports or compiles in driver code (JS
#    import/require, also via `new URL(...)`; Python import; Rust #[path]/include!) or
#    symlinks into tools/herdr/; its own manifests may run the driver from a script
#    entry (`node ../../tools/herdr/run.mjs ...`), any other tools/herdr reference there
#    fails. A crate's own tests/integration.rs is not the top-level directory.
#    Workflows (K6): no workflow other than
#    .github/workflows/herdr-provider-optin.yml names tools/herdr or any label a
#    self-hosted runner carries (self-hosted, oac-harness, linux, windows, macos, x64, arm,
#    arm64 -- a job routes to any runner holding all its runs-on labels). One exact line is
#    exempt (#345): a `run:` of `[$LOOPBACK_ONLY ]node tools/herdr/run.mjs --self-test`, the
#    driver's offline self-test against its test doubles (ci.yml `herdr-selftest`); any
#    other text on the line, a comment naming the path, or a run: block still fails, and
#    the whole parsed `run:` value must be exactly that command: a deeper-indented
#    continuation line, or the line inside a block, quoted or flow scalar, fails (PR #349
#    review B1). Its step may hold only name/id/if/shell/run/timeout-minutes, each once,
#    shell only `bash`; no job env:/defaults:/container:/services:, no workflow defaults:,
#    workflow env: CARGO_* only, and no YAML anchor, alias or merge key (#353; D2 holds the
#    same). Not statically checkable: a sibling step writing to `$GITHUB_ENV` (ci.yml sets
#    LOOPBACK_ONLY that way); that rests on review of the job. The opt-in
#    workflow is read with a small fail-closed YAML reader and must have: `on:` exactly
#    {workflow_dispatch, push} (an allowlist: issues/watch/fork/discussion/PR events all
#    fail), push limited to branches [main] and paths [docs/planning/PINS.md],
#    workflow_dispatch with inputs only, top-level permissions exactly contents: read and
#    no job-level permissions, every `uses:` actions/checkout or actions/upload-artifact
#    at a 40-hex commit SHA, checkout with persist-credentials: false; and must not have
#    any `secrets` use (secrets.X, secrets[..], toJSON(secrets), secrets: inherit), any
#    `github.token`, a `${{ }}` inside a `run:` block, or a direct driver call or driver
#    option. Text rules apply to comments too, so the opt-in workflow's own comments
#    avoid those words. All of this catches drift in this repository, not a fork: a
#    fork's pull request runs its own copy of every workflow and of this lint. The
#    runner-side pre-job hook is what refuses a fork (docs/planning/gates/herdr-runner.md
#    section 1).
# 10. The herdr driver must not touch harness credentials or harness config (boundaries 3,
#    4, 13) in entries under tools/herdr/: auth.json, .credentials.json, ANTHROPIC_API_KEY,
#    OPENAI_API_KEY, ANTHROPIC_AUTH_TOKEN, CLAUDE_CODE_OAUTH_TOKEN, CODEX_API_KEY,
#    keyring/keychain/OS credential-store access, `integration install`, and harness
#    config-mutating CLI calls (`claude|codex mcp add/remove`, `plugin install`,
#    `config set`, `login`, ...).
#    Both scan the git index (staged blobs, what CI sees), not the work tree. An entry is
#    matched by its path, a file by its blob, a symlink by its stored target and then by
#    where it lands after following tracked symlinks segment by segment, as realpath does
#    (chains and symlinked directories, each expanded before a following `..`; the landed
#    file's blob, or every tracked entry under a landed directory, each directory entered once per
#    top-level entry, so cycles and fan-out stay bounded), a
#    submodule by its path and .gitmodules url. A tracked symlink landing outside the repository fails. Hits print path, line,
#    and rule label only -- never matched text, a line, or a link target.
#    --self-test plants one violation per rule in a temporary git tree, asserts each exits
#    non-zero with exactly one hit and that planted secret strings never reach the output,
#    plus controls that must not fail.
#
#    Check 10's harness-config WRITE rules (~/.claude/settings(.local).json or under
#    $CLAUDE_CONFIG_DIR, ~/.codex/config.toml or under $CODEX_HOME, hooks.json directly in
#    either directory; each name suffix-anchored, so a scratch hooks.json.sha256 or a
#    hooks.json elsewhere is not config) are heuristics; reading or hashing that config
#    is allowed. Caught: JS writeFile/appendFile/
#    truncate/rm/unlink/createWriteStream on the path, copyFile/cp/link/symlink onto it,
#    rename from or onto it, open with a write flag -- each read by call arguments, so a
#    call split over lines counts; spawn/execFile of cp/mv/install/ln/rm/tee/truncate/sed/
#    perl with the path among its args; shell cp/install/ln onto it (last operand), mv/rm/
#    tee/truncate on it, sed -i/perl -i on it, and a `>`/`>>` redirect into it.
#    NOT caught: a config path held in a variable or built elsewhere and then passed to a
#    write (`const p = join(home, '.claude', 'settings.json'); writeFileSync(p, ...)`);
#    config dirs spelled any other way (incl. a plugin's hooks/hooks.json deeper under
#    ~/.claude); writes through another language or tool (python,
#    jq, dd, `node -e`); shell commands split across lines. Keep harness config paths
#    inline at the one read/hash site and never hand them to a write.
node scripts/check-herdr-containment.mjs
node scripts/check-herdr-containment.mjs --self-test

# 11. Beacon (agent-beacon, the external shared-memory tool, Epic L #165) runs beside OAC,
#    never inside it: no Beacon identifier, MCP tool name, store path, or dependency in any
#    git-tracked entry under adapters/ core/ cli/ transports/ spec/, or in a root
#    Cargo.toml / Cargo.lock (boundaries 2 and 12 -- OAC owning shared memory is context
#    management; boundary 3 -- no reading another tool's store or credentials). There is
#    no spec/ exception: Beacon guidance is docs-only, so spec/ text never names it.
#    Case-insensitive; a hit in a tracked path name or in file content fails. Scans the
#    git index (git ls-files), not the work tree. No tracked file in scope prints PENDING
#    and exits 0 -- pending is not a pass. CI runs this exact command as boundary-lint.yml
#    step "Check 11 - no Beacon in product paths"; change both together.
pattern='\bbeacon\b|beacon_|beacon-managed|agent-beacon|asymptote-labs|memory\.db|get_memory_context|search_memory|get_memory\b'
mapfile -d '' files < <(git ls-files -z -- adapters core cli transports spec Cargo.toml Cargo.lock)
if [ "${#files[@]}" -eq 0 ]; then echo "check 11 PENDING"; else
  printf '%s\n' "${files[@]}" | rg -n -i -e "$pattern"; pstatus=$?
  rg -H -n -i -e "$pattern" -- "${files[@]}"; status=$?
  [ "$pstatus" -eq 1 ] && [ "$status" -eq 1 ] && echo "check 11 clean" || echo "check 11 FAIL"
fi

# 12. Zenoh containment (boundary 5; DESIGN acceptance criterion 9; 07 section 4(a)): no
#    Zenoh name, `zid`, key expression or liveliness term in any git-tracked entry outside
#    transports/zenoh/ under core/ cli/ adapters/ transports/ spec/ tests/fakes/
#    tests/protocol/ (fixtures and contract suites) or the root Cargo.toml. Not Cargo.lock
#    (G-7 section 7): the zenoh crates in it are confined by check-crate-deps.mjs rule 4.
#    Wider than check 1, and the close of the scope gap 09 section 8 and C7 section 2
#    record: `zenoh` matches inside longer identifiers, `zid` as any snake/kebab/camel/
#    Pascal segment (leading too: `zidMap`, `ZidMap`), and adapters/ and cli/ are in
#    scope. The module's own name (oac-transport-zenoh, transports/zenoh) is the one
#    allowance. tests/security/ and tests/integration/ are out of scope (they compose a
#    real transport in Stages 4-5). Not proven by any text lint: an innocuous alias,
#    `concat!`/macro-pasted or run-time names, `\u{..}` escapes, homoglyphs.
# 13. Test doubles stay out of product code (PR #318 review item 9, the check 9 sibling):
#    no core/ cli/ adapters/ transports/ entry refers to tests/fakes/ or
#    tests/protocol/contract/ by path except a Cargo [dev-dependencies] entry (#[path],
#    include!, include_str!, JS imports and path strings fail; a crate's own tests/ or
#    benches/ file may name a fake by path to spawn it, never compile it in; leading
#    comment lines and Markdown are skipped). check-crate-deps.mjs rule 5 covers the
#    cargo graph.
#    Both scan the git index; a symlink or submodule in scope fails closed; an empty scope
#    is an error. CI: boundary-lint.yml job `containment`.
node scripts/check-containment.mjs
node scripts/check-containment.mjs --self-test
```

A clean run, as of the F1 scaffold (#50, 2026-10-05), is zero hits on checks 1 and 2 (on
`spec/` and `core/`), 3, 4, 5, 6, 7 and 8; `Result: CLEAN` (zero violations, no target
pending) on checks 9 and 10; and `check 11 clean` on check 11. A product path that is
removed again makes ripgrep error as missing, and that is pending, not a pass. Since #61
(2026-10-07) checks 12 and 13 also print `Result: CLEAN`.
