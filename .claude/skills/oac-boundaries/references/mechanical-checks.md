# Mechanical boundary checks

The one boundary lint script is `scripts/check-herdr-containment.mjs` (checks 9 and 10).
Checks 1-8 have no script: this file **is** the check — run the whole list as part of every
`type:code` / `type:spec` work item, not just once. (`scripts/check-skills.mjs` checks skill
budgets, not ADR-001 boundaries.) `.github/workflows/boundary-lint.yml` runs checks 3, 8, 9
and 10 on every push and pull request; checks 1, 2, 4-7 are not wired to CI and stay manual.

All commands are Git Bash / ripgrep syntax. `spec/`, `core/`, `adapters/`, `cli/`, and
`transports/zenoh/` do not exist yet in this repo (DESIGN §Suggested repository shape is a
sketch, not built) — ripgrep errors "cannot find the file specified" on a missing path.
That error is the expected, correct state today; it means the check is **pending**, not
passing. Re-run the whole list once code lands at those paths and treat any real match as a
stop-and-cite event, not a pending-path error.

Status as last verified against this repo (2026-09-28): checks 1, 2, 4, 5, 6, 7 are
**pending** (target paths do not exist yet); checks 3 and 8 are **clean** (zero hits) against
the current tree, which is docs/backlog plus `scripts/` and `tools/herdr/`; check 10 is
**clean** on real content (`tools/herdr/`, K3's driver: every tracked entry, 0 hits), and
so are check 9's two workflow targets (K6: `boundary-lint.yml` and
`herdr-provider-optin.yml`, 0 hits), while check 9's six other targets (the five product
paths and manifests outside `tools/herdr/`) are still **pending**, so the script's last
line reads `Result: PENDING`; its `--self-test` passes 62/62.

Checks 9 and 10 report pending themselves instead of via a ripgrep path error: a target with
no git-tracked files prints `PENDING`, the last line reads `Result: PENDING`, and the exit code
is 0 so CI stays green. `Result: PENDING` is still not a pass — only `Result: CLEAN` is. Any
violation exits 1 with `Result: FAIL`.

```bash
# 1. Zenoh vocabulary must not appear in the neutral spec or core types.
rg -n --glob '!target' -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness' spec/ core/

# 2. Provider-specific method names must not appear in neutral interfaces.
rg -n --glob '!target' \
  'claude/channel|thread/queue/add|turn/steer|turn/start|thread/start|thread/resume|notifications/claude/channel' \
  spec/ core/

# 3. No direct provider SDK imports/deps anywhere in the code tree (adapters call harness
#    IPC, not model APIs). Excludes docs/ and backlog JSON, which legitimately discuss these
#    names; language is still an open §5 decision, so cover Rust, Python, TS/JS, Go, and
#    manifest files rather than assuming one toolchain.
rg -n --glob '!target' --glob '!docs/**' -i \
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
rg -n -i --glob '!docs/**' 'dockerfile|docker-compose|kubernetes|helm|zenohd' .

# 9. herdr (Epic K dev/test tooling) must not reach product code: no `herdr` / `HERDR_`
#    (case-insensitive) in any git-tracked entry under adapters/ core/ cli/ transports/
#    spec/, and no workspace or package manifest outside tools/herdr/ referencing
#    tools/herdr. Workflows (K6): no workflow other than
#    .github/workflows/herdr-provider-optin.yml names tools/herdr, the `oac-harness`
#    runner label, or `self-hosted`; that opt-in workflow has no PR-event trigger, no
#    trigger chained from another workflow, a comment or a schedule, no secrets context,
#    no `${{ }}` inside a `run:` block, and no direct driver call or driver option.
#    Line-based text rules: they apply to comments too, so the opt-in workflow's own
#    comments avoid those words. They catch drift in this repository, not a fork: a
#    fork's pull request runs its own copy of every workflow (see
#    docs/planning/gates/herdr-runner.md).
# 10. The herdr driver must not touch harness credentials or harness config (boundaries 3,
#    4, 13) in entries under tools/herdr/: auth.json, .credentials.json, ANTHROPIC_API_KEY,
#    OPENAI_API_KEY, ANTHROPIC_AUTH_TOKEN, CLAUDE_CODE_OAUTH_TOKEN, CODEX_API_KEY,
#    keyring/keychain/OS credential-store access, `integration install`, and harness
#    config-mutating CLI calls (`claude|codex mcp add/remove`, `plugin install`,
#    `config set`, `login`, ...).
#    Both scan the git index (staged blobs, what CI sees), not the work tree. An entry is
#    matched by its path, a file by its blob, a symlink by its stored target and then by
#    where it lands after following tracked symlinks hop by hop (chains and symlinked
#    directories; the landed file's blob too), a submodule by its path and .gitmodules
#    url. A tracked symlink landing outside the repository fails. Hits print path, line,
#    and rule label only -- never matched text, a line, or a link target.
#    --self-test plants one violation per rule in a temporary git tree, asserts each exits
#    non-zero with exactly one hit and that planted secret strings never reach the output,
#    plus controls that must not fail.
#
#    Check 10's harness-config WRITE rules (~/.claude/settings(.local).json or under
#    $CLAUDE_CONFIG_DIR, ~/.codex/config.toml or under $CODEX_HOME, hooks.json) are
#    heuristics; reading or hashing that config is allowed. Caught: JS writeFile/appendFile/
#    truncate/rm/unlink/createWriteStream on the path, copyFile/cp/link/symlink onto it,
#    rename from or onto it, open with a write flag -- each read by call arguments, so a
#    call split over lines counts; spawn/execFile of cp/mv/install/ln/rm/tee/truncate/sed/
#    perl with the path among its args; shell cp/install/ln onto it (last operand), mv/rm/
#    tee/truncate on it, sed -i/perl -i on it, and a `>`/`>>` redirect into it.
#    NOT caught: a config path held in a variable or built elsewhere and then passed to a
#    write (`const p = join(home, '.claude', 'settings.json'); writeFileSync(p, ...)`);
#    config dirs spelled any other way; writes through another language or tool (python,
#    jq, dd, `node -e`); shell commands split across lines. Keep harness config paths
#    inline at the one read/hash site and never hand them to a write.
node scripts/check-herdr-containment.mjs
node scripts/check-herdr-containment.mjs --self-test
```

A clean run is zero hits on checks 3 and 8 today and `Result: PENDING` (zero violations) on
checks 9 and 10; checks 1, 2, 4, 5, 6 and 7 report the missing-path error until the
corresponding tree exists, at which point zero hits (and `Result: CLEAN`) is the bar.
