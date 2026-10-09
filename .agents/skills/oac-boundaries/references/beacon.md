# Beacon beside OAC, not inside it

Tier-3 reference for boundary #12 (shared context manager). Read it when a work item says
"integrate Beacon", "add shared memory", "let sessions remember each other", or touches
agent-beacon in any way. This file routes; it does not restate. The decision, pin, license,
threat rows, operator commands and lint live in the files linked below — read those, not a
copy here.

## The rule

Beacon (agent-beacon) is an external service each harness connects to **natively**, through
its own MCP configuration. OAC never launches, configures, proxies, calls, stores, fetches,
summarizes, or injects memory. A memory ID crosses OAC only as ordinary sender-written text,
and is untrusted content like any other. If a plan needs OAC to touch memory, it has left
the ADR-001 boundary — stop and cite per `SKILL.md` "Stop, cite the boundary".

## In bounds / out of bounds

- In-bounds points (seven): `docs/planning/decisions/L1-beacon-memory.md` §1 "Decision".
- Out-of-bounds list (boundary, not backlog): same file, §1 "Out of scope".
- Boundary lines it runs up against, quoted: same file, §1 "Boundary lines this decision
  runs up against".
- Rejected reading ("OAC as the shared context layer") and the amendment route: §5.
- Pin, license and UNVERIFIED items: §2, §3, §6 (status §11, live leg §12) — cite
  Beacon facts through L1, never re-derive or copy the pin here.

## Operator decisions Q1-Q5 (2026-09-29, not re-opened)

Full text and rationale: `L1-beacon-memory.md` §4.

| Q | Outcome |
|---|---|
| Q1 | Docs-only: memory IDs are plain `content[].text`; no spec content type; L3 closed with no spec change. |
| Q2 | No `oac doctor` check reading harness MCP configs. |
| Q3 | Docs recommend Beacon Local mode, or Metadata-only if hosted forwarding is on; local capture stays an open risk. |
| Q4 | Per-repository vs per-`working_directory` scoping mismatch: documented, not solved. |
| Q5 | No ADR-001 amendment; stay inside the boundary. |

## Drift examples

Each one is a boundary hit, not a feature request.

1. **OAC calls `get_memory_context`** (or any Beacon MCP tool) to enrich a message or pick a
   recipient. Context management and routing on memory: boundaries #2 and #12. The harness
   recalls in its own turn (L1 §1 point 2).
2. **Auto-attaching memory to envelopes**, or injecting recall results into inbound
   messages. Same boundaries; also puts memory where provenance could be confused with
   content (`06-security.md` §2, §14 rows 21-22).
3. **Using `runtime.jsonl` as a catch-up mailbox** (e.g. `search_activity` to replay missed
   messages). Deferred durable offline mailbox plus inbox polling: boundaries #11 and #14
   (L1 §1 point 6).
4. **Holding Beacon Cloud tokens** (hosted OAuth or personal tokens) in OAC, or reading
   harness MCP configs to find them. Boundary #3 (L1 §1 point 5, §4 Q2).
5. **Adding a memory content type to the spec**, or naming Beacon anywhere under `spec/`.
   Rejected by Q1 (L1 §4 Q1); check 11 has no `spec/` exception, and the neutral-vocabulary
   rule is `oac-spec-authoring`'s.

## Where the content lives

- Decision record: `docs/planning/decisions/L1-beacon-memory.md` (§1, §4, §5; pin §2,
  license §3, UNVERIFIED §6 (status §11, live leg §12)).
- Doctrine and threats: `docs/planning/v0.1/06-security.md` §2 (memory references are
  content), §13 (capture outside `working_directory` scoping), §14 rows 21-23.
  Normative, neutral form (no service named): `spec/security.md` §1.2, §9.3, §9.6
  (SEC-AUZ-024), §12.5 (SEC-PRV-015 to -017), §12.6, §13 memory rows.
- Risk register: `docs/planning/v0.1/11-risks.md` `RISK-BEACON`.
- Operator guide and "What OAC does not do": `docs/planning/v0.1/08-cli-and-deployment.md`
  §20 (all Beacon commands live there, not here).
- Lint: `references/mechanical-checks.md` check 11 (no Beacon in product paths;
  `node scripts/local-ci.mjs` step `boundary-check-11`).
