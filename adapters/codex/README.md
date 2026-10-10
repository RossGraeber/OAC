# Codex app-server client (G6, Refs #67)

This crate implements the client half only. It uses the documented app-server protocol
over a stream opened by `cli/`, with hand-written RFC 6455 framing and `oac-core` as its
only dependency. The experimental compatibility boundary is
`oac-codex-experimental@codex-0.161.0` in `src/shim.rs`. It does not implement
`ProviderAdapter`; delivery and attachment pairing belong to G7 and G8.

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
