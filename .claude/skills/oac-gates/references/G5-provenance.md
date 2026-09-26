# G5 — Provenance

Source: PLANNING-PROMPT.md §4 G5, §7. Backlog: `docs/planning/backlog/03-tasks-CD.json` D5.

## Disposition

No fallback stated in §4. Not explicitly listed as v0.1 go/no-go by name, but failure
invalidates DESIGN acceptance criterion 6 — treat a FAIL as blocking that criterion until
resolved.

## What the spike proves

A message whose body text claims a different sender is rendered to the model with
machine-set provenance that contradicts the claim, on both providers.

## Pass criteria (evaluate each individually)

- [ ] Claude: sender provenance arrives as `meta` attributes on the `<channel>` tag and
      **cannot be forged from message content** — a body claiming to be from someone else does
      not change the rendered `meta` attributes.
- [ ] Codex: the machine-generated header is visibly outside the delimited untrusted body (the
      model can distinguish header-asserted sender from body-claimed sender).
- [ ] In both cases the model is shown the contradiction rather than the claim alone — i.e. the
      spoofing attempt is observably distinguishable in what the model receives, not silently
      overwritten or silently accepted.
- [ ] A `meta` key that is not identifier-safe (not letters/digits/underscore) is confirmed to
      be silently dropped, and the spike confirms no security-relevant attribute (sender,
      device, session id) relies on such a key.

## Failure criteria

Provenance that cannot be distinguished from content invalidates DESIGN acceptance criterion
6. Any of the four unmet is a FAIL for that provider; report per-provider results since the
gate spans both.

## Fallback

None stated. If the spike fails on one provider, that provider's provenance rendering is a
finding requiring a design change before Stage 2 spec freeze, not a workaround.

## Surfaces and version pins

- Claude: `meta` is a string-to-string map on `notifications/claude/channel`; each key becomes
  an attribute on the `<channel>` tag Claude sees; keys must be identifier-safe or are silently
  dropped (§3.1). Legacy MCP negotiation applies here too, per G1.
- Codex: text-input framing — the OAC adapter constructs a machine-generated header delimited
  from the untrusted body when injecting via `thread/queue/add` / `turn/start` / `turn/steer`
  (§3.2, Decision 8). Codex version is floating (`docs/planning/PINS.md`, "Floating-version policy"); record
  the observed version. The original baseline was `@openai/codex` 0.154.0.
- This gate is the executable test behind PLANNING-PROMPT §7's security requirements:
  "Model-generated text never establishes identity; provenance is machine-set metadata
  separate from content on both providers," and "Anyone able to reply on a Claude channel can
  approve tool use if permission relay is enabled" (permission relay itself is proposed off by
  default in v0.1 per Decision 8/C10, not yet decided — out of scope for this gate's pass
  criteria, but do not enable it to make the spike easier).

## §3.1/§7 facts this spike must confirm or refute

- Confirms the `meta` identifier-safety silent-drop behavior from §3.1 is real, and — the part
  that matters for security — that no provenance-critical key is exposed to this silent-drop
  failure mode (a key with a typo or a disallowed character must not quietly become "no sender
  shown" for a security-relevant field).
- Confirms DESIGN acceptance criterion 6 (enforced provenance) is achievable given conflict
  **C4**: "DESIGN envelope `security.signature` is 'implementation-defined' while acceptance
  criterion 6 requires enforced provenance" — Zenoh has no payload signing (§3.4), so this gate
  is testing the *rendering* half of provenance; envelope signing itself is a separate Decision
  5 concern, not retested here.
- No §3 UNVERIFIED item is specifically assigned to G5 in STATUS.md; if the spike surfaces
  provider behavior not covered by §3.1/§3.2 (e.g. an edge case in header/body delimiting),
  record it as a new UNVERIFIED item.

## Fixtures to capture

Capture, for each provider, one exchange where the message body explicitly claims a false
sender and the rendered/injected form the model actually receives (the `<channel>` tag with
its `meta` attributes for Claude; the framed text with header and delimited body for Codex),
plus one exchange using a non-identifier-safe `meta` key to document the silent-drop. Pinned
version and date on each fixture.
