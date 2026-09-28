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
the current tree, which is docs/backlog plus `scripts/`; checks 9 and 10 are **pending** — the
script prints `PENDING` for all seven targets (no product path, no workspace manifest, no
`tools/herdr/` yet; K3 creates the driver) and its `--self-test` passes 37/37.

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
#    tools/herdr. An entry is matched by its path, a file by its content, a symlink by its
#    stored target (and the file it resolves to), a submodule by its path and .gitmodules url.
# 10. The herdr driver must not touch harness credentials or harness config (boundaries 3,
#    4, 13), matched the same way in entries under tools/herdr/: auth.json,
#    .credentials.json, ANTHROPIC_API_KEY, OPENAI_API_KEY, ANTHROPIC_AUTH_TOKEN,
#    CLAUDE_CODE_OAUTH_TOKEN, CODEX_API_KEY, keyring/keychain/OS credential-store access,
#    `integration install`, harness config-mutating CLI calls (`claude|codex mcp add`,
#    `plugin install`, `login`, ...), and a write to ~/.claude/settings(.local).json,
#    ~/.codex/config.toml or hooks.json. Reading/hashing harness config is allowed; the write
#    rule is a same-line heuristic, so do not route a config path through a variable into a
#    write call. One script runs both; hits print file:line and the rule, never the matched
#    line. --self-test plants one violation per rule in a temporary git tree and asserts each
#    exits non-zero with exactly one hit, plus controls that must not.
node scripts/check-herdr-containment.mjs
node scripts/check-herdr-containment.mjs --self-test
```

A clean run is zero hits on checks 3 and 8 today and `Result: PENDING` (zero violations) on
checks 9 and 10; checks 1, 2, 4, 5, 6 and 7 report the missing-path error until the
corresponding tree exists, at which point zero hits (and `Result: CLEAN`) is the bar.
