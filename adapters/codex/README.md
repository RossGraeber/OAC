# Codex app-server client (G6, Refs #67), the pairing internals (G8 S3) and the adapter (G8 S4, Refs #69, #403, #68)

This crate implements the client half, the internals of the Codex issued-value pairing (G8
slice S3) and, from slice S4, `CodexAdapter`, its `ProviderAdapter`. It uses the documented app-server protocol over a stream opened by
`cli/`, with hand-written RFC 6455 framing, `oac-core` and `oac-mcp-tools` as its OAC
dependencies, and `rmcp` and `tokio` as the vetted MCP server side (the same lines as
`adapters/claude`). The experimental compatibility boundary is
`oac-codex-experimental@codex-0.161.0` in `src/shim.rs`. `adapter::CodexAdapter`
implements `pairing::CallPort`, the one seam between the pairing internals and the core.

## Pairing internals (`spec/bindings/mcp.md` §4.5)

| Module | What it does |
|---|---|
| `mcp` | The OAC MCP server for one Codex stdio connection, on `rmcp`: legacy era only, the four tools of `oac_mcp_tools::tools()` (so Codex's `tools/list` is Claude's, [MCPB-TOOL-003]), no channel capability and no notification of any kind ([MCPB-CDX-001]). The `tools/call` `_meta` is not read. |
| `pairing` | `CodexPairing`: for each call, either the unpaired route (serve threads, ask the core, keep a record of a refusal that carries a pairing value) or the paired route (wait for a confirmation, then ask the core). `CallPort` is the seam to the provider adapter: pass the call to the core's sink, report `attachment-unconfirmed` before the request it concerns ([IFC-ADP-091]), report a reveal as a native signal. |
| `reveal` | The refused-call records and the reveal match: a completed `mcpToolCall` of the registered server whose tool and arguments match a record and whose `result.content` is the refusal's content. The thread is the notification's `threadId` only ([MCPB-ATT-008]). |
| `confirm` | The confirmation engine: live `item/started` entries keyed by thread and item id, a bounded wait (5 s) in either order, single use ([MCPB-ATT-014]), end on `item/completed`, `turn/completed` or 600 s ([MCPB-ATT-021]); argument matching. |
| `serving` | `thread/loaded/list` (ids only; `thread/list` is never called) and `thread/resume` for each new loaded thread before a pairing refusal goes out; unsubscribes the unpaired ones when the window ends. |

The registered name is a setting, not a constant: `Config::new(name)` (`Config::try_new`
checks it with `validate_registered_name`), `DEFAULT_REGISTERED_NAME` (`oac`) when nothing
else is said. It must equal the name given to `codex mcp add <name>`, because Codex reports
it as the `server` of each call and a report under another name pairs nothing. The daemon sets
it with `OAC_CODEX_REGISTERED_NAME` (`docs/planning/v0.1/08-cli-and-deployment.md` §9). Such
a report is counted (`PairingDiagnostics::other_server`) and, with `Config::with_log`, named
once per distinct name in the log (the names only, never a thread id or a pairing value).

The core issues the values, ends the windows, uses each value once and caps the windows at
three ([IFC-ADP-090] to [IFC-ADP-093], `oac_core::issued_pairing`). This crate does what only
the adapter can: it sees Codex's own reports on the carrier.

Matching, as the spec states it and as the S1 capture (`docs/planning/gates/fixtures/
g8-codex-tools/`) bears out: equal JSON values, `null` and absent the same, `{}` only `{}`.
Codex 0.163.0 sent `{}` and reported `{}` for a call the model made with no arguments, so
that case is an ordinary match. A call sent with no `arguments` member and reported as `{}`
(or the reverse) does not match: it is refused and not paired (availability only), and
`tests/pairing.rs` pins it as the spec's strictness. If a Codex version does report that,
the spec's matching rule needs a change request.

`_meta.itemId` (Codex 0.163.0) is a `ctc_...` id that no notification carries; the item's own
id is `exec-<uuid>` and equals `_meta.callId`, a value the caller wrote. Neither is read, so
neither strengthens the match (see the module documentation of `confirm`).

The approval step of a call (`thread/status/changed` `waitingOnApproval`,
`mcpServer/elicitation/request`, `serverRequest/resolved`) lies between `item/started` and
the call (S1 C5). The pairing reads only `item/started`, `item/completed` and
`turn/completed`, and the client leaves every server request unanswered, so none of it is in
the way; the wait for a confirmation is bounded by 5 s from the call, not from the approval,
so a call is served only if its approval was answered before the call reached the server,
which is when Codex makes it.

## The adapter (G8 slice S4; G7's hand-off)

| Module | What it does |
|---|---|
| `adapter` | `CodexAdapter`: `take_connection`, events, `set_binding`, `capabilities`, `deliver`, `accept_requests`, `health`, `shutdown`; the `CallPort` the pairing talks to |
| `frame` | The C6 §5.0 frame a hand-off is written as: provenance header, then a delimited, line-quoted, untrusted body |

- **Two kinds of connection.** The carrier is only the handle the daemon designates with
  `CodexAdapter::designate_carrier(handle)`, before it gives the connection over
  (`Pipelines::connect_carrier`); never in-band ([MCPB-ATT-006]). Every other connection is a
  shim connection: one attachment, `attachment-opened` after the first `tools/list` answer.
  With no carrier up, a shim connection is closed; while the carrier connects, shim
  connections wait. One carrier serves an adapter at a time; a lost one is replaced by a new designation (S5).
- **Pairing.** A reveal is reported as a `native-signal` on the carrier (`native_id` the
  notification's `threadId`, `revealed` the value, start kind `fresh`). The thread is kept per
  attachment, uncommitted. `set_binding(Some)` commits it and calls
  `CodexPairing::set_bound(true)`; `set_binding(None)` clears the thread and the reveal. A
  request the pairing cannot confirm is reported as `attachment-unconfirmed` first
  ([IFC-ADP-091]). The core records a finding for every such event, also for an attachment it
  has not bound ([IFC-ADP-093] is unconditional; `core/tests/pipeline.rs`
  `an_unconfirmed_event_for_an_unbound_attachment_is_a_finding_without_a_binding_to_end`
  pins that this binds, ends and withholds nothing).
- **Delivery.** One `thread/queue/add` per hand-off for the committed thread, `clientUserMessageId`
  from the client, nothing else; never `turn/start` or `turn/steer`; no server request is
  answered. `completed` for a result with `queuedSubmission`; `failed` for a JSON-RPC error
  (every known refusal is `handoff-failed`, §8.2.1) and for a call not sent; `indeterminate` for
  no answer, an answer lost with the carrier (`ClientError::LostAnswer`) or a stream error;
  `refused` for a provenance value that is not a whole token; never `not-now`.
- **An unprompted `thread/archived`** (and the `notLoaded` status before it, S3 capture
  L1024-L1029; the fake's `oacFake/thread/setArchived` now sends both) changes nothing: no
  event, no capability change, no binding change. The attachment is the MCP connection, whose
  end is the stream's end. The next delivery makes its one add, gets the recorded archived
  refusal and is `handoff-failed`; unarchived, the next completes.
  `tests/adapter.rs` pins this.
- **Subscription ownership.** Delivery never subscribes. The thread of a delivery is the thread
  the attachment's reveal named, which `Serving` holds for that attachment from the reveal
  until it is unbound or ends; a window's end unsubscribes only unheld tentative threads.
  A delivery whose thread is no longer served is `failed` without a call.
- **Carrier lost.** `capabilities` reports `active_inbound: false` and `capabilities-changed` is
  reported for each open attachment; deliveries fail; `health` is no longer healthy.
- **Not done.** Interfaces 0.6's cooperative local-call surface is not implemented (the
  legacy surface, which the pipeline accepts, is). **Mid-turn subscription (UNVERIFIED until S6):**
  whether a `thread/resume` issued while the thread's MCP call is in flight still gets that
  item's `item/completed` was shown by no capture until the S6 live leg (S1 subscribed before the call). *Answered 2026-10-29
  (G8 S6, Codex `0.165.0`, once): yes.* A second connection's `thread/resume`, answered 493 ms after the call started and 373 ms before it
  completed, received that item's `item/completed`, and the adapter's own mid-turn resume paired
  (the G8 S6 run record under `docs/planning/gates/`, claim 5). If a Codex version stops doing so, pairing costs availability only.
  The S6 live leg also ran the harness's sequence against a real Codex and a real Claude Code
  (pairing, a send, hand-offs, replies; the fake replays the S1 frames and does not run Codex).

Tests: unit tests in each module; `tests/pairing.rs` against the real node fake app-server
process with its `oacFake/mcp/*` frames and a stand-in core built on `PairingWindows`;
`tests/adapter.rs` for `CodexAdapter` against the same fake and a stand-in core that binds on a
reveal; and `tests/contract.rs`, the shared adapter contract suite run unchanged.

`Options::call_timeout` bounds the HTTP Upgrade and each JSON-RPC response wait. The
Upgrade accepts at most 16 KiB of headers. On any connect error, the stream owner must
shut down the underlying stream, including cloned halves, to reclaim a worker blocked
in I/O. A late Upgrade cannot initialize a client after the deadline. The client installs
no read timeout on the carrier, so normal idle periods remain supported.

Three consecutive unanswered calls make health `Degraded`; a matched response resets
that count. This reports lack of progress, without claiming to identify its cause.
`is_open()` still reports the carrier's observed connection state. Debug formatting of
events and completed items hides all content, including MCP arguments, results and
pairing values.

## Approval evidence

The adapter counts server requests and writes no answer, including no error or decline.
For ordinary thread tool approvals, Codex sends the request to every connection
subscribed to that thread. One callback belongs to the request id: the first answer
resolves it. Thus the TUI can answer while the adapter stays silent; an adapter decline
would decide the request itself. If this carrier is the only subscriber and nobody else
answers, the approval remains pending until a turn-state change aborts it (or a later
subscriber answers). OAC does not supply that decision.

Source: [thread listener](https://github.com/openai/codex/blob/rust-v0.161.0/codex-rs/app-server/src/request_processors/thread_lifecycle.rs)
(`subscribed_connection_ids`, `ThreadScopedOutgoingMessageSender::new`), and
[outgoing requests](https://github.com/openai/codex/blob/rust-v0.161.0/codex-rs/app-server/src/outgoing_message.rs)
(`send_request`, `send_request_to_connections`, the request-id callback map, response
callback removal, `abort_pending_server_requests`), version `rust-v0.161.0`, retrieved
2026-10-10. This describes tool approvals, not the separately owner-bound user-verification
elicitation path.

No recorded app-server fixture contains an approval request, and the F9 fake does not
emit one. The consequence above is source-confirmed, not runtime-confirmed by a fixture.
`server_requests_are_counted_and_never_answered` uses parser-level test inputs solely to
prove OAC's no-answer invariant; it does not add a synthetic harness fixture.

The other review regression tests cover a silent Upgrade peer, oversized headers, the
cumulative 16 MiB fragment cap (17 frames of 1 MiB), an unoffered subprotocol, a one-byte
close payload, degraded health and recovery, and private event Debug output. These run
in the default tier with no live harness. The security mutation runner currently targets
core mitigations and the security suite, which cannot reach this client-only crate;
the two review mutations are checked against this crate's unit tests directly.

## For S5 (the daemon's wiring; done in S5)

- **One carrier at a time; a lost carrier is replaced (S5, `docs/planning/v0.1/11-risks.md` row 90).** `designate_carrier` is refused while a carrier is connecting or up, and accepted again after `CarrierState::Lost` (the start failed, or the connection ended: `carrier_state()`). The daemon (`cli/src/codex.rs`, `CarrierDriver`) connects a carrier at start if the control socket is there and when a Codex shim connects with none or a lost one, never on a timer, and never starts or stops Codex's app-server. Shims served through a lost carrier keep their place until their streams end and hand nothing off.
- **Order.** The daemon must designate the carrier (`CodexAdapter::designate_carrier(handle)`) and hand it over with `Pipelines::connect_carrier` **before** any shim connection arrives: with no carrier a shim connection is closed, and while the carrier connects it waits.
- **Surface.** The adapter uses the legacy event surface; interfaces 0.5/0.6's cooperative local-call surface is not implemented for Codex.
