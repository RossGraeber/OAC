# Conformance fixture format — detail

A fixture is a data file, not code, that proves exactly one normative requirement and is
readable by a second, independent implementation (task E8 acceptance).

## Required content

Each fixture must contain:
- the spec clause it proves — a stable requirement id (see "Requirement-id scheme" below),
  never a line number, which drifts;
- the input — a serialized envelope, a signature vector, or a timing/nonce sequence;
- the expected outcome — accept/reject, the resulting delivery state, or the emitted error
  code;
- for a negative fixture, which failure mode it exercises.

## Required negative-fixture set

Task E8 acceptance requires all four of these to exist as negative fixtures:
- a forged signature;
- an expired timestamp;
- a replayed nonce;
- an unknown version.

## Location and execution

Fixtures live under `tests/protocol/` (DESIGN §Suggested repository shape). A conformance
runner, built in Stage 3 (task F12 wiring), executes them in CI.

## Requirement-id scheme

Not yet defined. Task E1 fixes the envelope and versioning spec first, and the requirement-id
scheme is part of that freeze — it is the natural place to assign stable ids to every `MUST`
across the spec surface, since it also defines what a breaking change to those ids would mean
(§6 of `SKILL.md`). Until task E1 lands: no fixture may cite a requirement id, because there
is no scheme yet to cite one against. A fixture written before E1 lands is incomplete — hold
it as a draft, not as a landed conformance fixture, until it can name a real id.
