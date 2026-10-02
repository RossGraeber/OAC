# Gate write-up pitfalls

Recurring mistakes from G2 and G4 review rounds. Check these before sending a gate
result for review, not after the first round of comments finds them.

## Timebox discipline

- Declare the timebox, with a UTC timestamp, in the session log immediately before each
  live session — including a resumed session after a break. A box declared once at the
  start of a multi-session spike does not cover a later resume.
- Evidence captured outside any declared box cannot support a PASS. If it lands after
  the box expired, record that run as a superseded `NOT RUN` and re-run inside a fresh
  box (G4: evidence arrived ~21h after the box expired and had to be redone this way).
- Before closing a box, run every probe still on the pending list, or record it as not
  attempted and say why. Do not let a probe fall off the list silently (G4: the Codex
  `mcp_2026_07_28` probe was left unattempted with ~51 minutes of box remaining).

## Re-run layout

When a gate result records a superseded run followed by a re-run:

- The current run's template fields — Sources, UNVERIFIED, Fixtures, Pin rows relied on,
  `PINS.md` as-of, Re-run history — sit outside any "superseded" subsection. A reviewer
  reading the top-level fields must see the current run's data, not the stale run's.
- Keep the superseded run's own Date, Timebox, and Pinned-version fields in its
  subsection so cross-references (e.g. "see the 2026-… run") still resolve.
- Cite only committed artifacts or the session log. Never cite an uncommitted revision
  as "git history" — if it isn't committed, it isn't history yet.

## Criterion scope

- Quote the gate reference's surrounding text, not just the checkbox, when a criterion
  names a specific provider or client. The checkbox alone can read as satisfied by the
  wrong client.
- State which client actually satisfied the criterion.
- Don't resolve a conflict-register or ADR row unless every element of that row's
  statement is evidenced — partial evidence closes nothing.

## Change evidence

"Not in the diff" proves nothing when `gh api compare` is capped at 300 files. Fetch and
diff the file at both commits directly instead of trusting a truncated compare view.

## Derived docs

Any doc that claims to match STATUS.md cell for cell must be synced in the same change:
`docs/planning/v0.1/02-gating-findings.md`, and every file that mentions the gate's
verdict (grep for it before closing).

## What worked

Capturing the full version triple (CLI, daemon, and wire `clientInfo`/user-agent)
in-run, plus redaction reproducible byte-for-byte from a script, made verification fast
and repeatable in review. Keep doing this.
