# Neutral-vocabulary check — spec-specific additions

Companion to `oac-boundaries` "Mechanical checks" 1-2, which already grep
`\bzenoh\b|\bzid\b|key[_-]?expr|liveliness` and the provider method names
(`claude/channel`, `thread/queue/add`, `turn/steer`, `turn/start`, `thread/start`,
`thread/resume`, `notifications/claude/channel`) over `spec/ core/`. Run those first. This
file adds only what they miss, per PLANNING-PROMPT.md §11 item 6 ("No neutral interface
mentions Zenoh, Claude, Codex, MCP method names, or key expressions").

## Additions this skill carries

- `mqtt`, `nats`, `scouting` — not in `oac-boundaries` check 1.
- Bare `\bclaude\b` and `\bcodex\b` — `oac-boundaries` check 2 only bans specific method
  strings, not the bare provider names §11 item 6 also forbids.
- `app[ -]server` — matches both `app-server` and the space form `app server`; a plain
  `app-server` pattern misses the latter (verified: ripgrep 15.2.0 gives zero hits on the
  space form against a plain `app-server` regex).
- MCP method-name forms: `tools/call`, `prompts/get`, `resources/read`, `notifications/…/…`
  — §11 item 6 bans "MCP method names" generically, not only the Claude-specific ones
  `oac-boundaries` already covers, and `thread/loaded/list` (missing from `oac-boundaries`
  check 2's thread-method list). Bare `initialize` and bare `notifications/` are excluded
  here: both fire on ordinary neutral-spec English ("MUST initialize the session",
  "Presence notifications/ are delivered") and move to the read-the-hit group below.

## Combined command (Git Bash / ripgrep)

`spec/` exists (#41) and `node scripts/local-ci.mjs` runs the zero-hits group below
(section `boundary-checks-1-2` of `.github/local-ci.sh`, copied verbatim from the deleted
`boundary-lint.yml` when GitHub-hosted CI went on 2026-10-08; the source of truth: if it
changes, change this block too). The exemption is task-scoped (§3 of `SKILL.md`) and names
one exact regular file, the task E6 MCP binding `spec/bindings/mcp.md`, not a directory: any
other file under `spec/bindings/` is scanned like the rest of `spec/`. It passes `rg` an
explicit file list (every regular file under `spec/` from `find`, minus that one path) with
`--hidden --no-ignore`, so dot-files and ignored files stay in scope and no glob can drop
one. It also fails if `spec/bindings/mcp.md` exists but is not a regular file, or if anything
under `spec/` is a symlink.

Two groups, like `oac-boundaries` "Mechanical checks" 6: a **zero-hits group**, where any hit
is a violation, and a **read-the-hit group**, where a hit must be read before it is treated as
guilty, because the pattern also matches ordinary English.

```bash
binding=spec/bindings/mcp.md
mapfile -d '' spec_all < <(find spec -type f -print0)
spec_neutral=()
for f in "${spec_all[@]}"; do [ "$f" = "$binding" ] || spec_neutral+=("$f"); done

# Zero-hits group: any hit here is a boundary violation (rg exit 1 = clean).
rg -n --hidden --no-ignore -i \
  '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness|scouting|\bmqtt\b|\bnats\b|\bclaude\b|\bcodex\b|app[ -]server|claude/channel|--channels|--dangerously-load-development-channels|thread/(queue/add|start|resume|loaded/list)|turn/(steer|start)|tools/call|prompts/get|resources/read|notifications/[a-z]+/' \
  -- "${spec_neutral[@]}"

# Read-the-hit group: a hit is read, not assumed guilty — "initialize" and "notifications/"
# alone both appear in ordinary normative prose (e.g. "MUST initialize the session",
# "Presence notifications/ are delivered actively"), the same way oac-boundaries treats a
# hit on its polling-loop check (Mechanical checks 6) as read, not assumed guilty.
rg -n --hidden --no-ignore -i '\binitialize\b|notifications/' -- "${spec_neutral[@]}"
```

A clean run is zero hits from the first command. Any hit there, in normative text or a
neutral interface signature, is a boundary violation — follow the `oac-boundaries`
"Stop, cite the boundary" protocol. A hit from the second command is read in context: a hit
that names the MCP `initialize` handshake or an MCP `notifications/…` method is a violation;
ordinary use of "initialize" or "notifications" as English words is not.

## A missing path is not a pass

If a path a check names does not exist, ripgrep exits 2 with an I/O error (verified: ripgrep
15.2.0, Git Bash). That is a failure, not zero hits. `spec/` exists (#41) and `local-ci.mjs` fails if it
is missing; for any other path a check names before it is built, re-run the command once the
path exists, per `oac-boundaries` "Mechanical checks".
