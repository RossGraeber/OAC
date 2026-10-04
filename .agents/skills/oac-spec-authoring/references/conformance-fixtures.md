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

Fixtures live under `tests/protocol/` (DESIGN §Suggested repository shape). The reference
runner `node tests/protocol/runner/run.mjs` (E8, #48; CI job `conformance`) evaluates every
fixture from the spec text and checks the requirement indexes against the fixtures. Run it
in any change that adds a fixture, a stage or an index row; a new stage needs an evaluator
there. Driving the workspace's own code through the fixtures is task F12.

## Requirement-id scheme

Defined by task E1 (#41) in `spec/session-channels.md` §3.2 (`<DOC>-<AREA>-<NNN>`, e.g.
`SC-ENV-010`; never renumbered or reused; a change of meaning gets a new id). The fixture
file layout and JSON members are in §3.3, and the id-to-fixture index is its Appendix A.
Read them there; they are not restated here. A spec task adds its ids to Appendix A and its
fixtures under `tests/protocol/<doc>-<area>/` in the same change.
