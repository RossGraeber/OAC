---
name: oac-evidence
description: Evidence standard — first-party sources, URL+version+retrieval-date citations, verbatim API naming, UNVERIFIED labelling. Load for nearly every work item: type:docs, type:decision, type:spec, type:code on a provider surface, any claim-bearing artifact.
---

Load whenever you are about to write a claim about Claude Code, Codex, MCP, Zenoh,
ACP, or Cursor behavior — which is nearly every work item. Companion guardrail:
`oac-boundaries` (ADR-001 MUST NOTs). That skill covers boundaries; this one covers
evidence. Do not restate its content here or vice versa.

## 1. First-party-source rule

Cite only: official provider docs, the provider's own source repository, and its
release notes. Blog posts, Stack Overflow, and third-party wrappers may inform your
search but never decide a claim — if a fact's only backing is a blog post, it is not
verified, it is a lead. Go find the first-party doc or source line, or label the
fact `UNVERIFIED` (§5).

## 2. Citation format

Every externally verifiable claim carries URL + version-or-commit + retrieval date.
Template:

```
<claim>. Source: <URL>, version/commit <X>, retrieved <YYYY-MM-DD>.
```

Worked example (imitate this shape exactly):

```
A channel is an MCP server that declares
capabilities.experimental["claude/channel"] = {}. Source:
https://code.claude.com/docs/en/channels.md, Claude Code v2.1.232+,
retrieved 2026-09-15.
```

If you are citing a fact already recorded in `docs/planning/PLANNING-PROMPT.md` §3,
you may cite the section instead of re-quoting the URL: "per PLANNING-PROMPT.md
§3.1, retrieved 2026-09-15" — but only if you have not changed the claim. If you
add a new claim not in §3, use the full template above and record it in
`docs/planning/v0.1/01-capability-matrix.md` once Stage 0 (§8) has produced that
file; until then, record it in `docs/planning/STATUS.md`.

## 3. Verbatim-naming rule

Method names, capability keys, flags, and file paths are quoted exactly from the
source — same case, same punctuation, same brackets. Never paraphrase an API name
and never invent one that "sounds right" for the pattern.

Practical test before writing any such name: **if you cannot point at the line in
a first-party source you copied it from, it is invented.** Stop and go find the
line, or mark the surrounding claim `UNVERIFIED`.

Examples of the rule already applied correctly (do not deviate from this casing):
`capabilities.experimental["claude/channel"]`, `notifications/claude/channel`,
`thread/queue/add`, `turn/steer`, `_meta["io.modelcontextprotocol/protocolVersion"]`,
`--dangerously-load-development-channels`.

## 4. Surface-labelling rule

Every provider surface gets exactly one label: `supported`, `research preview`,
`experimental`, or `undocumented`. State the label next to the first mention of the
surface in any artifact.

- `research preview` and `experimental` surfaces get two additional things: a named
  compatibility shim boundary (the module/interface that isolates the volatile
  surface from the rest of OAC) and a pinned version. See `oac-boundaries` for how
  the shim boundary itself must be enforced (never restated here).
- Do not upgrade a label based on how stable a surface "feels." Use the label the
  source itself states (e.g. PLANNING-PROMPT.md §3.1 states Claude Channels is
  "research preview"; §3.2 states the Codex daemon-attach path is "experimental").

## 5. UNVERIFIED labelling

A fact qualifies as `UNVERIFIED` when: no first-party source states it directly, it
is inferred or estimated, or it was true as of the retrieval date but has not been
re-checked against the currently pinned version.

- Write it as: `<claim> (UNVERIFIED — <one-line reason>)`.
- It must also appear in `docs/planning/STATUS.md` (the "Open UNVERIFIED items"
  list) and, once the risks document exists, `docs/planning/v0.1/11-risks.md`. Do
  not let an UNVERIFIED fact exist in only one place.
- **`docs/planning/STATUS.md` is the ledger of record.** An UNVERIFIED fact is
  never silently promoted: promotion to a stated fact requires re-verifying
  against a first-party source, recording that verification (§2 format) at the
  point of promotion, and — in the same change — removing the entry from
  STATUS.md's "Open UNVERIFIED items" list and updating its Pins row if a
  version was involved (see §7 step 6). Deleting the label without touching
  STATUS.md is an incomplete promotion.
- The current open items are listed in `docs/planning/STATUS.md` under "Open
  UNVERIFIED items" — read that file rather than trusting a copy of the list here;
  it is updated as Stage 0 closes items.

## 6. Precedence chain

`ADR-001` > `DESIGN` > PLANNING-PROMPT.md §3 baseline > your judgment — **except**
that verified evidence beats all three. "Verified" means it satisfies §1 and §2
above, not "I found something that seems to contradict this."

When evidence invalidates a settled decision:

1. Do **not** silently redesign around it.
2. Record the conflict: what the decision said, what the evidence says, where each
   lives (file + section).
3. Cite the evidence per §2.
4. Propose a numbered ADR amendment. Before choosing a number, check both
   PLANNING-PROMPT.md Appendix A's "Expected resolution" column (which already
   earmarks `ADR-001-A1` for C1, `A2` for C2, `A3` for C3) and
   `docs/planning/v0.1/03-decisions-and-amendments.md` for numbers already
   allocated there; take the next number above every number found in either
   place. Write it with three fields: old text (verbatim), new text, rationale.
5. Land the amendment in `docs/planning/v0.1/03-decisions-and-amendments.md` once
   that file exists; until then, record it in the work item and flag it in
   `STATUS.md`.

This is the same procedure PLANNING-PROMPT.md §1 and §11 item 1 require of the
plan itself — it does not relax for skills, code, or tests.

## 7. Re-verification procedure

PLANNING-PROMPT.md §3 is **starting evidence, not permanent truth** — it is a
pre-verified baseline retrieved 2026-09-15 (per `SKILLS-MODEL.md` and
`docs/planning/STATUS.md`). Re-verify when either trigger fires:

- **A pin moves** (`docs/planning/STATUS.md` Pins table changes for any surface).
- **A Stage 0 task runs** (Epic B in the backlog).

Procedure:

1. Take the exact §3 fact and its cited URL/source.
2. Fetch or inspect the source at the new pinned version.
3. If the fact still holds verbatim: re-cite with the new version and today's date
   (§2). No drift.
4. If the fact changed: record the drift (old value, new value, source, date),
   update every skill and reference file that stated the old value, and check
   whether the drift invalidates a decision (§6).
5. If a surface skill (`oac-claude-channels`, `oac-codex-appserver`, `oac-mcp`,
   `oac-zenoh`) carries this fact, update its `## Pin` section, not just the prose.
6. If the fact was labelled `UNVERIFIED` and step 3 or 4 resolved it: in the same
   change, remove its entry from `docs/planning/STATUS.md`'s "Open UNVERIFIED
   items" list, and update the Pins table row for that surface if a version was
   involved. STATUS.md is the ledger of record for this — see §5.

## 8. Pre-submission checklist

Run before submitting any claim-bearing artifact (docs, spec text, skill body,
commit message describing behavior):

- [ ] Every externally verifiable claim has URL + version/commit + retrieval date,
      or cites a PLANNING-PROMPT.md §3.x subsection that already carries them.
- [ ] Every quoted method name, capability key, flag, or path is copied verbatim
      from a source you can point at — none invented.
- [ ] Every provider surface mentioned is labelled supported / research preview /
      experimental / undocumented.
- [ ] Preview/experimental surfaces name their compatibility shim boundary and
      pinned version.
- [ ] Every unverifiable claim is labelled `UNVERIFIED` with a reason, and appears
      in `docs/planning/STATUS.md` (and `11-risks.md` once it exists).
- [ ] No UNVERIFIED label was silently dropped without a re-verification citation.
- [ ] Any conflict with ADR-001/DESIGN/§3 found during this work is recorded per
      §6, not quietly resolved by rewriting the decision.

## Where the content lives

- Evidence standard, source: `docs/planning/PLANNING-PROMPT.md` §10, §11 item 5.
- Precedence and amendment procedure: `docs/planning/PLANNING-PROMPT.md` §1.
- Pre-verified baseline: `docs/planning/PLANNING-PROMPT.md` §3.
- Conflict register seed: `docs/planning/PLANNING-PROMPT.md` Appendix A.
- Source index (Appendix B, ellipses expanded): `.claude/skills/oac-evidence/references/source-index.md`.
- Current stage, pins, and open UNVERIFIED items: `docs/planning/STATUS.md`.
- Boundary guardrail (companion skill, not restated here): `oac-boundaries`.
