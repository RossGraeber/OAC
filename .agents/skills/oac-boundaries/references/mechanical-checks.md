# Mechanical boundary checks

No boundary-specific lint exists in this repo. The only lint script,
`scripts/check-skills.mjs`, checks skill frontmatter/line budgets, not ADR-001 boundaries.
Until a boundary lint is added, this file **is** the check — run it as part of every
`type:code` / `type:spec` work item, not just once.

All commands are Git Bash / ripgrep syntax. `spec/`, `core/`, `adapters/`, `cli/`, and
`transports/zenoh/` do not exist yet in this repo (DESIGN §Suggested repository shape is a
sketch, not built) — ripgrep errors "cannot find the file specified" on a missing path.
That error is the expected, correct state today; it means the check is **pending**, not
passing. Re-run the whole list once code lands at those paths and treat any real match as a
stop-and-cite event, not a pending-path error.

Status as last verified against this repo (2026-09-16): checks 1, 2, 4, 5, 6, 7 are
**pending** (target paths do not exist yet); checks 3 and 8 are **clean** (zero hits) against
the current tree, which is docs/backlog only.

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
```

A clean run is zero hits on checks 3 and 8 today; checks 1, 2, 4, 5, 6 and 7 report the
missing-path error until the corresponding tree exists, at which point zero hits is the bar.
