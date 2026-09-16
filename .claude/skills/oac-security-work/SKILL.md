---
name: oac-security-work
description: Threat table template, threat inventory, the "authenticated but untrusted" doctrine, and provenance rendering rules. Load for area:security work items, or tasks touching signing, replay, pairing, authorization, permission relay, turn/steer, or local IPC peer auth.
---

Loaded for `area:security` work items (PLANNING-PROMPT.md §7; DESIGN "Security"; ADR-001
"Security model"). This skill holds the threat-modeling procedure and the doctrine that
governs it. It does not restate `oac-boundaries` (ADR-001 MUST NOTs), `oac-evidence`
(sourcing standard), `oac-testing` (test tiers), or the surface skills' protocol detail
(`oac-claude-channels`, `oac-codex-appserver`, `oac-zenoh`) — load those separately when the
task needs them; link here instead of copying.

## 1. Threat table template

Copy this row shape verbatim into any threat table (`06-security.md`, a decision doc, a
gate result, a PR description). PLANNING-PROMPT.md §7: "For each threat: attack,
precondition, mitigation, which test proves the mitigation, and residual risk."

```markdown
| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| <what an attacker does> | <what must be true first> | <the control that stops it> | <named test, e.g. F11 security suite / H2 case> | <what remains even if the mitigation holds> |
```

**Rule: a mitigation with no proving test is not a mitigation.** A row with "proving test"
left blank, marked "TBD", or pointing at a test that does not exist yet is not a finished
threat-table row — it is an open risk. Record it as a risk (`11-risks.md` once it exists,
or the work item) instead of a closed mitigation. This is the same discipline H5 enforces
at the end: "Every threat-table mitigation names the test that proves it, and that test
passes. Any unproven criterion is called out as a v0.1 gap rather than quietly marked done."

## 2. Threat inventory — cover every row

Every threat table an agent writes must cover the DESIGN "Security" list plus each §7
addition. Treat this as a checklist, not a sample.

From DESIGN "Security" (threats include):
- [ ] Impersonation
- [ ] Unauthorized routing/discovery
- [ ] Tampering
- [ ] Replay
- [ ] Malicious peer prompt injection
- [ ] Compromised transport infrastructure
- [ ] Accidental cross-project disclosure
- [ ] Leaked credentials

From PLANNING-PROMPT.md §7 additions (quoted/near-verbatim, with the exact flags and method
names that must appear unparaphrased):
- [ ] "Zenoh has no payload authentication, so envelope signatures are the only authenticity
      proof; Zenoh `zid` is not an identity." — the transport layer performs no payload
      authentication of its own; do not describe any Zenoh-layer property as authenticating
      a sender. DESIGN's envelope still reads `"signature": "implementation-defined"` — that
      is the known conflict (Appendix A C4), not the rule: task C5 resolves it as "Signature
      is normative, not 'implementation-defined' (resolves C4)." Write and treat the
      envelope signature as normative.
- [ ] "Anyone able to reply on a Claude channel can approve tool use if permission relay is
      enabled." — this is the `claude/channel/permission` capability named in
      PLANNING-PROMPT.md §3.1; see §5 below for its default.
- [ ] "Codex `turn/steer` writes into an in-flight turn; unauthorized steer is a
      code-execution risk." — `turn/steer` is the exact method name (PLANNING-PROMPT.md
      §3.2); any code path that calls it must be gated by an explicit authorization check
      (see task G7, `docs/planning/backlog/05-tasks-GHIJ.json`: "`turn/steer` is either
      unused or gated behind an explicit authorization check").
- [ ] "The `--dangerously-load-development-channels` confirmation is the only user consent
      step on the Claude side until OAC is on an allowlist; the plan must not weaken it." —
      quote the flag verbatim; never propose automating past, suppressing, or pre-answering
      that interactive confirmation.
- [ ] "Model-generated text never establishes identity; provenance is machine-set metadata
      separate from content on both providers." — see §3 (doctrine) and §5 (rendering)
      below.
- [ ] "Local IPC between shims and daemon must authenticate the peer process (Windows
      named-pipe security descriptors, Unix socket permissions)." — both mechanisms named
      verbatim; a design that skips peer authentication on either platform does not satisfy
      this row (see task G9, `docs/planning/backlog/05-tasks-GHIJ.json`).
- [ ] "Cross-project leakage: a session in one working directory must not be discoverable by
      a peer that is not authorized for it." — also task H2's fourth acceptance item
      (`docs/planning/backlog/05-tasks-GHIJ.json`).

A threat table that omits any checked item above is incomplete, regardless of how many
other rows it has.

## 3. Doctrine: authenticated but untrusted

State this explicitly in any security-facing spec text, design note, or code comment that
touches message handling — an agent must not be able to miss it:

> An authenticated peer message is still an untrusted instruction and may carry prompt
> injection. Authentication answers "who sent it." It never answers "should this be obeyed."
> (ADR-001 "Security model": "Authenticated peer messages remain untrusted instructions and
> may contain prompt injection." DESIGN "Security": "malicious peer prompt injection" is a
> named threat regardless of authentication.)

Practical consequence for **code**: a verified signature, a matched allowlist entry, or a
successful pairing check authorizes *routing and delivery* of a message to a session. It
never authorizes an *action* the message's content asks for (approving a tool call,
steering a turn, changing configuration) without a separate, explicit authorization
decision. Do not let "signature verified" and "instruction obeyed" collapse into one check.

Practical consequence for **spec text**: never write a sentence of the shape "because the
sender is authenticated, X is safe to do automatically," where X is anything other than
delivering the message into the addressed session's input. If a spec section grants an
authenticated sender any additional power (e.g. tool approval), it must say so as a
separate, named authorization decision with its own default (see §5's permission-relay
note), not as a side effect of authentication.

## 4. Identity hierarchy and default posture

ADR-001 "Security model":

```text
Security principal -> Device -> Harness -> Session
```

A session's authority derives from this chain, not from anything the message content
claims. Security layers named in ADR-001: authenticated/encrypted transport, default-deny
ACL/policy, cryptographic message authenticity, replay protection, optional later E2E
encryption.

**Default-deny is the standing posture.** DESIGN "Security": "Default deny." Any new
routing rule, ACL subject, or authorization path an agent adds must start denied and be
opened explicitly — never start open and get restricted later. This applies to Zenoh ACL
subjects, per-channel sender allowlists, and the authorization engine (F5) alike. On Zenoh
only certificate common names and usernames are authenticated (§5 decision 6); ACL subjects
must be drawn only from those, never from `zid` (task G3: "ACL subjects are authenticated
ones only, never `zid`").

## 5. Provenance rendering

Provenance is machine-set metadata about a message's sender, device, session, and target —
kept structurally separate from the message's content field and never derivable from or
overridable by that content, on both providers. ADR-001: "Provenance must remain
machine-enforced and distinct from message content."

Per-provider requirement, at the level PLANNING-PROMPT.md §5 decision 8 sets it (do not
restate the protocol mechanics — that detail lives in the named surface skill):
- **Claude:** sender, device, session, message id, and reply target arrive as `meta`
  attributes the harness attaches to the `<channel>` tag — never inside the message text
  the model reads as content. Security consequence to hold here (mechanics deferred to
  `oac-claude-channels`): a non-identifier-safe `meta` key is **silently dropped**, so no
  security-relevant provenance attribute may depend on a key that isn't identifier-safe.
- **Codex:** inbound text-input framing carries a machine-generated header, with the
  untrusted message body kept in a clearly delimited section separate from that header.
  Protocol detail (turn/thread mapping, framing mechanics): `oac-codex-appserver`.

**Permission relay is off by default in v0.1.** Appendix A C10: "Claude permission relay
lets any allowlisted sender approve tools" → "Off by default in v0.1." Task G4's acceptance
reads "Permission relay is off by default"; task H2's reads "Permission relay is confirmed
off, and the consequence of enabling it is documented" — build to G4's wording, verify to
H2's. Any work item that turns it on needs its own decision, not a silent flip.

**Delivery-receipt honesty (§5 decision 5):** receipts distinguish "accepted by adapter,"
"handed to harness," and "unknown" — never overstate them. A resolved Claude notification
send is "handed to harness," never "seen by the model" (task G4 acceptance).

## 6. Relationship to Gate G5

Provenance is a claim until G5 has run. G5's pass criterion (PLANNING-PROMPT.md §4): "A
message whose text claims a different sender is rendered to the model with machine-set
provenance that contradicts the claim, on both providers." Per `docs/planning/STATUS.md`,
**G5 verdict is `NOT RUN`** as of this writing. Do not write or accept a security work item
that asserts provenance is "proven" or "working" before G5 has a recorded PASS — cite the
current verdict from STATUS.md rather than assuming it. If your work item depends on G5
having passed, check STATUS.md first; if it still reads NOT RUN, that dependency is blocked
(see `oac-gates` for gate procedure).

## 7. Exit criteria for a security work item

- [ ] Every threat table row uses the §1 template with all five columns filled.
- [ ] No row's "proving test" column is blank, "TBD", or references a test that does not
      exist — such a row is moved to the risk list instead of left as a closed mitigation.
- [ ] The §2 inventory checklist is covered: the DESIGN list and every §7 addition appear,
      with flags and method names quoted verbatim (`zid`, `turn/steer`,
      `--dangerously-load-development-channels`, named-pipe security descriptors / Unix
      socket permissions).
- [ ] Any text describing authentication states the §3 doctrine's consequence and does not
      let a signature/allowlist check stand in for a separate authorization decision.
- [ ] Default-deny is the stated starting posture for any new ACL, allowlist, or
      authorization path (§4).
- [ ] Provenance is described as machine-set metadata distinct from content, at the
      per-provider level in §5, with protocol detail deferred to the named surface skill —
      not restated here.
- [ ] Permission relay is off by default; the work item does not enable it without its own
      explicit decision record.
- [ ] Any claim that provenance or a threat mitigation is "proven" cites the actual gate
      verdict or test result rather than an assumption — check `docs/planning/STATUS.md` for
      G5 before asserting provenance is settled.

## Where the content lives

- `docs/planning/PLANNING-PROMPT.md` §7 (threat list and additions, quoted above), §5
  decisions 5-8 (envelope authenticity/replay, pairing/authorization, key storage,
  provider-facing trust rendering), §4 G5, §9 item 7, Appendix A C4 and C10.
- `docs/planning/DESIGN.md` "Security" (threat list, default deny, pairing/trust) and the
  envelope's `security` block.
- `docs/planning/ADR-001.md` "Security model" (identity hierarchy, authenticated-but-
  untrusted doctrine, provenance-vs-content separation).
- `docs/planning/backlog/02-tasks-AB.json` A7, `03-tasks-CD.json` C4/C5/C6/D5,
  `04-tasks-EF.json` E5/F3/F4/F5/F11, `05-tasks-GHIJ.json` G3/G7/G9/H2/H5 — the concrete
  work items this skill governs.
- `docs/planning/STATUS.md` — current G5 verdict and pins.
- `oac-boundaries`, `oac-evidence`, `oac-testing`, `oac-gates`, `oac-claude-channels`,
  `oac-codex-appserver`, `oac-zenoh` — link, do not restate.
