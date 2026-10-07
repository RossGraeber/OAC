// SPDX-License-Identifier: Apache-2.0

//! The security suite against the fakes (#60, F11).
//!
//! Every test in `tests/` proves one mitigation of the threat table
//! (`docs/planning/v0.1/06-security.md` §14, cited as "06 row N"; `spec/security.md` §13 for
//! the rows 06 does not number) and names that row in its own name and doc comment. The
//! mapping from each row to its tests, and what is still gated, is [`THREATS`];
//! `tests/threat_map.rs` checks it against the test sources, so a row cannot point at a test
//! that does not exist.
//!
//! # What runs
//!
//! Each test drives the real core, never a re-implementation of it: envelope-stage
//! validation and the security stage in Table 7.1 order
//! ([`oac_core::receiver::receive_octets`]), authorization
//! ([`oac_core::authorization::AuthorizationEngine`]), the duplicate store, presence and
//! receipt authentication, pairing and key removal. Envelopes cross the in-memory transport
//! where the threat sits on the carrying path, and the fake Claude Code endpoint shows what
//! the harness renders. It is the CI-default tier: no live provider, no API key and no
//! network beyond loopback (`docs/planning/v0.1/09-test-strategy.md` §3).
//!
//! # What is gated
//!
//! Some mitigations live in components that do not exist yet: the provider adapters (Epic
//! G), the daemon and its local IPC (G9, #70), and the composed send and receive pipelines
//! (#313). A test for such a mitigation is `#[ignore = "GATED on #N ..."]`, and its body
//! fails if it is run: it is never a pass. [`THREATS`] lists each one with its owning issue.
//!
//! # The stand-in provenance mapping
//!
//! No adapter exists yet, so nothing in the repository turns a verified message into the
//! Claude channel `meta` map. [`stand_in_provenance`] does it here, from the verified members
//! only (`spec/security.md` §12.1, [SEC-PRV-002]), so that the core's half (what it verifies
//! and hands over) and the harness's half (what the fake renders) can be tested together. It
//! is a stand-in for the G4 adapter (#65), not that adapter: a pass says nothing about G4's
//! own mapping, which [`THREATS`] lists as gated.

use std::collections::BTreeMap;
use std::sync::Arc;

use oac_core::adapter::HandOffOutcome;
use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
use oac_core::clock::{Clock, ManualClock};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::{
    ChannelMessage, ContentPart, Envelope, EnvelopeDraft, EnvelopeLimits, TextPart,
};
use oac_core::ids::{EXTENSION_ID_V0, KeyId, SessionId, Timestamp, Token, Version};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::presence::PresenceRecord;
use oac_core::receiver::{
    DeliveryTarget, ReceiptLimiter, ReceiptRefusal, ReceiveOutcome, Received, ReceiverReport,
    may_send_receipt, receive_octets,
};
use oac_core::registration::RegistrationRecord;
use oac_core::replay::DuplicateStore;
use oac_fake_claude::{Config, FakeClaude, MidTurnRelease, Phase};

/// The instant every test starts at.
pub const T0: &str = "2026-10-07T12:00:00Z";

/// A timestamp.
pub fn ts(s: &str) -> Timestamp {
    Timestamp::parse(s).expect("a timestamp")
}

/// A session id built from one repeated octet, so tests can name sessions by number.
pub fn sid(n: u8) -> SessionId {
    SessionId::from_random_octets([n; 16])
}

/// An identifier token.
pub fn token(s: &str) -> Token {
    Token::parse(s).expect("an identifier token")
}

/// A new device identity with a fresh key.
pub fn identity(principal: &str) -> DeviceIdentity {
    DeviceIdentity::new(DeviceKey::generate(), token(principal))
}

/// A shared manual clock at [`T0`].
pub fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(ts(T0)))
}

/// The text of every `text` part of `msg`, joined by newlines.
pub fn text_of(msg: &ChannelMessage) -> String {
    msg.envelope()
        .content()
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text(t) => Some(t.text().to_owned()),
            ContentPart::Other(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a receiving device did with one copy, as its octets arrived.
#[derive(Debug)]
pub struct Delivery {
    /// The receiver-observed state.
    pub state: DeliveryState,
    /// Its code, when the state carries one.
    pub error: Option<ErrorCode>,
    /// The key id the copy verified under at security steps 1 and 2.
    pub verified_by: Option<KeyId>,
    /// Whether step 4 refused it with a binding finding ([SEC-PRS-004]).
    pub finding: bool,
    /// Whether the delivery stage looked the addressed session up at all.
    pub looked_up: bool,
    /// What reached the adapter's hand-off call, if anything did.
    pub handed: Option<ChannelMessage>,
    /// The receiver's report, for the receipt decision.
    pub report: Option<ReceiverReport>,
}

impl Delivery {
    /// The state and code together.
    pub fn outcome(&self) -> (DeliveryState, Option<ErrorCode>) {
        (self.state, self.error)
    }

    /// Whether the receiver may send a receipt for this copy now
    /// ([`oac_core::receiver::may_send_receipt`]).
    pub fn receipt_decision(
        &self,
        limiter: &mut ReceiptLimiter,
        now: std::time::Instant,
    ) -> Result<(), ReceiptRefusal> {
        let report = self.report.expect("a reported copy");
        may_send_receipt(self.verified_by.as_ref(), &report, limiter, now)
    }
}

/// One device: its identity, its authorization engine, its duplicate store, its pairing
/// store, its decision log and its own sessions, all on one manual clock. A test plays the
/// operator (pairing, grants) and the adapter (the hand-off call) around it.
pub struct Device {
    /// The device identity.
    pub identity: DeviceIdentity,
    /// The authorization engine (trusted keys, grants, bindings, records).
    pub engine: AuthorizationEngine,
    /// The duplicate store of security step 5.
    pub duplicates: DuplicateStore,
    /// The pairing store the engine saves to.
    pub pairings: MemoryPairingStore,
    /// The engine's decision log.
    pub log: MemoryDecisionLog,
    /// The receiver clock.
    pub clock: Arc<ManualClock>,
    /// The receiver-wide envelope limits.
    pub limits: EnvelopeLimits,
    own: Vec<RegistrationRecord>,
    targets: BTreeMap<SessionId, DeliveryTarget>,
}

impl Device {
    /// A device for `principal` with a fresh key, on `clock`, with no configuration: only
    /// its own key is trusted and every decision is `deny`.
    pub fn new(principal: &str, clock: Arc<ManualClock>) -> Device {
        Device::with_identity(identity(principal), clock)
    }

    /// A device for `identity`, on `clock`, with no configuration.
    pub fn with_identity(identity: DeviceIdentity, clock: Arc<ManualClock>) -> Device {
        let log = MemoryDecisionLog::new();
        let engine = AuthorizationEngine::new(&identity, clock.clone(), Box::new(log.clone()));
        Device {
            duplicates: DuplicateStore::new(clock.clone()),
            identity,
            engine,
            pairings: MemoryPairingStore::new(),
            log,
            clock,
            limits: EnvelopeLimits::default(),
            own: Vec::new(),
            targets: BTreeMap::new(),
        }
    }

    /// The receiver clock's reading.
    pub fn clock_now(&self) -> Timestamp {
        self.clock.now()
    }

    /// This device's key id.
    pub fn key_id(&self) -> KeyId {
        self.identity.key_id().clone()
    }

    /// The operator pairs `peer` by comparing its key id ([SEC-KEY-032]).
    pub fn pair(&mut self, peer: &DeviceIdentity) {
        let p = PairedPeer::by_key_id_comparison(
            peer.principal().clone(),
            *peer.public_key(),
            peer.key_id(),
            self.clock.now(),
            OperatorConfirmed::by_operator(),
        )
        .expect("the compared key id matches");
        self.engine.pair(p, &self.pairings).expect("paired");
    }

    /// Registers an own session under working directory `wd`, accepting input.
    pub fn register(&mut self, session: &SessionId, wd: &str) {
        let record = self
            .identity
            .register(
                session.clone(),
                token("harness"),
                "native-1",
                wd,
                self.clock.now(),
            )
            .expect("a registration record");
        assert!(self.engine.register_session(&record, &self.identity));
        self.own.push(record);
        self.targets.insert(
            session.clone(),
            DeliveryTarget {
                accepting: true,
                active_inbound: true,
                content_types: Vec::new(),
                max_envelope_octets: None,
            },
        );
    }

    /// An own session ends: its binding, its reply rights and its hand-off target go.
    pub fn end_session(&mut self, session: &SessionId) {
        assert!(self.engine.end_session(session));
        self.targets.remove(session);
        self.own.retain(|r| r.session_id() != session);
    }

    /// The operator adds `grant` ([SEC-AUZ-005]).
    pub fn grant(&mut self, grant: Grant) {
        self.engine
            .add_grant(grant, OperatorConfirmed::by_operator(), &self.pairings)
            .expect("grant saved");
    }

    /// The operator lets the device `peer` (any of its sessions) write to own session `to`.
    pub fn allow_from(&mut self, peer: &KeyId, to: &SessionId) {
        self.grant(Grant::Inbound {
            writer: PeerSide::device(peer.clone()),
            target: LocalSide::Session(to.clone()),
        });
    }

    /// The operator lets own session `from` write to any session of the device `peer`.
    pub fn allow_to(&mut self, from: &SessionId, peer: &KeyId) {
        self.grant(Grant::Outbound {
            writer: LocalSide::Session(from.clone()),
            target: PeerSide::device(peer.clone()),
        });
    }

    /// Signs a text envelope from `from` to `to`, created now on this device's clock, with
    /// a `ttl_ms` of 300000.
    pub fn sign(&self, id: &str, from: &SessionId, to: &SessionId, text: &str) -> Envelope {
        self.identity
            .sign_envelope(draft(id, from, to, self.clock.now(), text))
    }

    /// Signs a text envelope from `from` to `to`, created at `created_at`, with no `ttl_ms`,
    /// so that only the replay window bounds it.
    pub fn sign_at(
        &self,
        id: &str,
        from: &SessionId,
        to: &SessionId,
        created_at: Timestamp,
        text: &str,
    ) -> Envelope {
        let d = EnvelopeDraft::new(
            token(id),
            from.clone(),
            to.clone(),
            created_at,
            vec![TextPart::new(text).expect("a text part")],
        )
        .expect("a draft");
        self.identity.sign_envelope(d)
    }

    /// One copy, as its octets arrived, through every check a receiver makes; a copy that
    /// reaches the hand-off call is handed off with `completed`.
    pub fn receive(&mut self, octets: &[u8]) -> Delivery {
        self.receive_with(octets, HandOffOutcome::Completed)
    }

    /// As [`Device::receive`], with the hand-off call returning `outcome`.
    pub fn receive_with(&mut self, octets: &[u8], outcome: HandOffOutcome) -> Delivery {
        let Device {
            engine,
            duplicates,
            clock,
            limits,
            targets,
            ..
        } = self;
        let mut looked_up = false;
        let mut handed = None;
        let out: ReceiveOutcome = receive_octets(
            octets,
            limits,
            engine,
            duplicates,
            &**clock,
            |s| {
                looked_up = true;
                targets.get(s).cloned()
            },
            |m| {
                handed = Some(m.clone());
                outcome
            },
        );
        let report = match out.received {
            Received::Reported(r) => r,
            Received::InFlight(_) => panic!("no copy is in flight in a single-threaded test"),
        };
        Delivery {
            state: report.state(),
            error: report.error(),
            verified_by: out.verified_by,
            finding: out.finding,
            looked_up,
            handed,
            report: Some(report),
        }
    }

    /// A restart: the engine is rebuilt from the pairing store (paired keys, grants, relay
    /// settings), and everything held in memory only is gone: the duplicate store, the
    /// binding table and the sent and hand-off records ([SEC-RPL-025]). Own sessions
    /// register again.
    pub fn restart(&mut self) {
        self.engine = AuthorizationEngine::restore(
            &self.identity,
            &self.pairings,
            self.clock.clone(),
            Box::new(self.log.clone()),
        )
        .expect("restored");
        self.duplicates = DuplicateStore::new(self.clock.clone());
        for r in &self.own {
            assert!(self.engine.register_session(r, &self.identity));
        }
    }
}

/// A text draft from `from` to `to`, created at `created_at`, with a `ttl_ms` of 300000.
pub fn draft(
    id: &str,
    from: &SessionId,
    to: &SessionId,
    created_at: Timestamp,
    text: &str,
) -> EnvelopeDraft {
    EnvelopeDraft::new(
        token(id),
        from.clone(),
        to.clone(),
        created_at,
        vec![TextPart::new(text).expect("a text part")],
    )
    .expect("a draft")
    .with_ttl_ms(300_000)
    .expect("a ttl")
}

/// Two devices on one clock, each paired with the other: `alice` with session `sid(1)`
/// under `/work/a`, `bob` with session `sid(2)` under `/work/b`. No grant yet.
pub fn paired_pair() -> (Device, Device) {
    let c = clock();
    let mut alice = Device::new("alice", c.clone());
    let mut bob = Device::new("bob", c);
    alice.register(&sid(1), "/work/a");
    bob.register(&sid(2), "/work/b");
    alice.pair(&bob.identity);
    bob.pair(&alice.identity);
    (alice, bob)
}

/// [`paired_pair`] with the grants for `sid(1)` on alice to write to `sid(2)` on bob.
pub fn granted_pair() -> (Device, Device) {
    let (mut alice, mut bob) = paired_pair();
    alice.allow_to(&sid(1), &bob.key_id());
    bob.allow_from(&alice.key_id(), &sid(2));
    (alice, bob)
}

/// A session descriptor for `session` declaring revision 0.1 of the extension with active
/// inbound.
pub fn descriptor(session: &SessionId) -> SessionDescriptor {
    let caps = SessionCapabilities::declare([(
        EXTENSION_ID_V0,
        CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
    )])
    .expect("a declaration");
    SessionDescriptor::new(session.clone(), caps, None, None).expect("a descriptor")
}

/// An announcement of `session` with `seq`, issued at `issued_at`, for `lifetime_ms`.
pub fn announcement(
    session: &SessionId,
    seq: u64,
    issued_at: Timestamp,
    lifetime_ms: u64,
) -> PresenceRecord {
    PresenceRecord::announcement(seq, issued_at, lifetime_ms, descriptor(session))
        .expect("an announcement")
}

/// `octets` with the first occurrence of `from` replaced by `to`: a byte rewrite on the
/// carrying path. Panics when `from` does not occur, so a test cannot rewrite nothing.
pub fn rewrite(octets: &[u8], from: &str, to: &str) -> Vec<u8> {
    let s = std::str::from_utf8(octets).expect("an envelope is UTF-8");
    assert!(s.contains(from), "{from:?} does not occur in the envelope");
    s.replacen(from, to, 1).into_bytes()
}

// ---- Provenance: the stand-in mapping and the fake Claude Code endpoint ---------------

/// The Claude channel `meta` keys of the provenance set (`spec/security.md` §12.1; Decision
/// C6 §2): sender, device, session, message id and reply target.
pub const PROVENANCE_KEYS: [&str; 5] = [
    "oac_sender",
    "oac_device",
    "oac_session",
    "oac_message_id",
    "oac_reply_to",
];

/// The provenance set of `msg` as a channel `meta` map, taken only from the verified
/// envelope's header members and the key it verified under, never from `content`
/// ([SEC-PRV-002]). `None` for a message that has not passed security steps 1 and 2.
///
/// A **stand-in** for the G4 adapter's mapping (#65): see the crate documentation.
pub fn stand_in_provenance(msg: &ChannelMessage) -> Option<Vec<(String, String)>> {
    let by = msg.verified_by()?;
    let env = msg.envelope();
    let values = [
        env.from().as_str().to_owned(),
        by.key_id().as_str().to_owned(),
        env.to().as_str().to_owned(),
        env.id().as_str().to_owned(),
        env.reply_to()
            .map_or_else(String::new, |r| r.as_str().to_owned()),
    ];
    Some(
        PROVENANCE_KEYS
            .iter()
            .map(|k| (*k).to_owned())
            .zip(values)
            .collect(),
    )
}

/// A JSON string literal for `s`.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A `notifications/claude/channel` frame with `content` and `meta`, in order.
pub fn channel_notification(content: &str, meta: &[(String, String)]) -> String {
    let meta = meta
        .iter()
        .map(|(k, v)| format!("{}:{}", json_string(k), json_string(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{{\"content\":{},\"meta\":{{{meta}}}}}}}",
        json_string(content)
    )
}

/// The fake Claude Code endpoint, loaded as `server:oac` with mid-turn release `release`,
/// opened on the legacy era with a channel registered (the recorded opening: a refused
/// probe, `initialize` at `2025-11-25`, `tools/list`).
pub fn ready_claude(release: MidTurnRelease) -> FakeClaude {
    let mut fake = FakeClaude::new(
        Config::new("oac")
            .expect("a load name")
            .with_release(release),
    );
    assert_eq!(fake.take_outbound().len(), 1, "the server/discover probe");
    fake.receive(
        r#"{"jsonrpc":"2.0","id":"server-discover-probe-1","error":{"code":-32601,"message":"Method not found"}}"#,
    );
    fake.take_outbound();
    fake.receive(
        r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-11-25","capabilities":{"experimental":{"claude/channel":{}},"tools":{}},"serverInfo":{"name":"oac","version":"0"}}}"#,
    );
    fake.take_outbound();
    fake.receive(
        r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"reply","inputSchema":{"type":"object"}}]}}"#,
    );
    assert_eq!(fake.phase(), Phase::Ready, "{:?}", fake.halted());
    fake
}

// ---- The threat map ------------------------------------------------------------------

/// Where a threat row stands in this suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Every mitigation the row names that lives in an existing component is proven by a
    /// passing test here; `gated` lists the parts that wait for a component not built yet.
    Proven,
    /// Nothing the row names can run yet; `gated` says why.
    Gated,
    /// No mitigation exists to test: an open risk (06 §15).
    OpenRisk,
}

/// A part of a mitigation that waits for a component not built yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    /// The owning issue, as `#N`.
    pub issue: &'static str,
    /// What waits, and the ignored test that holds the place, if any.
    pub what: &'static str,
}

/// One threat row and the tests that prove its mitigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Threat {
    /// `06-N` for row N of `docs/planning/v0.1/06-security.md` §14; `S13-<name>` for a row
    /// of `spec/security.md` §13 that 06 does not number.
    pub row: &'static str,
    /// The attack, in a few words.
    pub attack: &'static str,
    /// The passing tests, by function name.
    pub tests: &'static [&'static str],
    /// The status.
    pub status: Status,
    /// What is gated, with its owning issue.
    pub gated: &'static [Gate],
}

const G4: &str = "#65";
const G7: &str = "#68";
const G8: &str = "#69";
const G9: &str = "#70";
const G1_G3: &str = "#62, #64";
const PIPELINES: &str = "#313";
const BINDINGS: &str = "#325";
const L10: &str = "#175";
const H2: &str = "#74";

/// Every threat row this suite covers. `tests/threat_map.rs` checks that each of 06 rows 1
/// to 24 is here, that every named test exists in `tests/`, and that every gated test is
/// ignored with its issue.
pub const THREATS: &[Threat] = &[
    Threat {
        row: "06-1",
        attack: "Impersonation: a forged envelope",
        tests: &[
            "row01_envelope_signed_by_an_unpaired_key_is_rejected",
            "row01_claiming_a_trusted_principal_with_another_key_is_rejected",
            "row01_a_trusted_key_id_with_a_forged_signature_is_rejected",
            "row01_signature_from_another_trusted_device_does_not_verify",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-2",
        attack: "Unauthorized routing or discovery",
        tests: &[
            "row02_trusted_peer_without_a_grant_is_rejected_unauthorized",
            "row02_a_grant_is_one_way",
            "row02_a_grant_for_one_session_does_not_cover_another",
            "row02_unauthorized_peer_cannot_discover_a_session",
            "row02_unauthorized_peer_cannot_address_a_session_through_presence",
            "row02_reply_right_covers_only_the_reply_to_the_one_message",
            "gated_row02_unauthorized_send_through_the_composed_pipeline",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: PIPELINES,
            what: "the same refusal through the composed send and receive pipelines over a transport (gated_row02_unauthorized_send_through_the_composed_pipeline)",
        }],
    },
    Threat {
        row: "06-3",
        attack: "Tampering in flight",
        tests: &[
            "row03_rewriting_any_signed_member_breaks_the_signature",
            "row03_a_flipped_signature_bit_is_rejected",
            "row03_tampered_bytes_over_the_transport_are_rejected",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-4",
        attack: "Replay",
        tests: &[
            "row04_copy_outside_the_replay_window_is_rejected",
            "row04_copy_dated_ahead_of_the_window_is_rejected",
            "row04_replay_inside_the_window_is_a_duplicate_handed_off_once",
            "row04_expiry_is_checked_before_the_replay_window",
            "row04_replay_after_restart_inside_the_window_is_the_named_residual",
            "row04_transport_duplicates_are_handed_off_once",
            "row04_receipt_flooding_by_replay_is_bounded",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-5",
        attack: "Prompt injection from an authenticated peer",
        tests: &[
            "row05_hostile_content_is_delivered_as_content_and_obeyed_never",
            "row05_content_never_reaches_an_authorization_decision",
            "row05_the_decision_log_holds_no_content",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-6",
        attack: "Compromised transport infrastructure",
        tests: &[
            "row06_forged_envelope_injected_on_the_transport_is_rejected",
            "row03_tampered_bytes_over_the_transport_are_rejected",
            "row04_transport_duplicates_are_handed_off_once",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G1_G3,
            what: "the same cases over the real reference transport on loopback (G1 transport, G3 security configuration)",
        }],
    },
    Threat {
        row: "06-7",
        attack: "Accidental cross-project disclosure",
        tests: &[
            "row07_scope_grant_does_not_cover_another_working_directory",
            "row07_scope_grant_does_not_cover_a_subdirectory",
            "row07_presence_is_released_only_to_granted_devices",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: H2,
            what: "H2's live cross-project check against real adapters",
        }],
    },
    Threat {
        row: "06-8",
        attack: "Leaked device key",
        tests: &[
            "row08_removing_a_key_revokes_it_in_one_step",
            "row08_captured_envelope_from_a_removed_key_is_rejected",
            "row08_removal_survives_a_restart",
            "row08_a_failed_save_still_revokes",
            "row08_own_key_cannot_be_removed",
            "row08_device_key_never_appears_in_debug_output",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-9",
        attack: "Transport-only authenticity assumed",
        tests: &[
            "row09_a_payload_from_any_endpoint_is_judged_by_its_signature_alone",
            "row06_forged_envelope_injected_on_the_transport_is_rejected",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-10",
        attack: "A transport peer identifier used as an identity",
        tests: &["row09_a_payload_from_any_endpoint_is_judged_by_its_signature_alone"],
        status: Status::Proven,
        gated: &[Gate {
            issue: "#64",
            what: "ACL subjects of the reference transport's configuration (G3); grants are keyed by key id only, by construction",
        }],
    },
    Threat {
        row: "06-11",
        attack: "Permission-relay abuse",
        tests: &[
            "row11_relay_is_off_by_default_even_with_a_device_wide_grant",
            "row11_relay_is_enabled_for_one_session_only_by_the_operator",
            "row11_a_deliver_permit_never_permits_relay",
            "gated_row11_adapter_never_relays_without_a_relay_permit",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G4,
            what: "the Claude adapter's permission relay surface stays off (gated_row11_adapter_never_relays_without_a_relay_permit)",
        }],
    },
    Threat {
        row: "06-12",
        attack: "Steering a running turn",
        tests: &[
            "row12_no_decision_kind_enables_steering",
            "row12_hand_off_is_made_at_most_once_and_no_outcome_steers",
            "gated_row12_codex_hand_off_is_queue_only",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G7,
            what: "the Codex adapter hands off queue-only and never calls a steering method (gated_row12_codex_hand_off_is_queue_only)",
        }],
    },
    Threat {
        row: "06-13",
        attack: "Local IPC peer spoofing",
        tests: &["gated_row13_ipc_admits_only_the_same_user"],
        status: Status::Gated,
        gated: &[Gate {
            issue: G9,
            what: "the daemon's local IPC endpoint and its OS peer check (gated_row13_ipc_admits_only_the_same_user)",
        }],
    },
    Threat {
        row: "06-14",
        attack: "Cross-project leakage through discovery",
        tests: &[
            "row02_unauthorized_peer_cannot_discover_a_session",
            "row14_discovery_lists_only_sessions_the_requester_may_reach",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: H2,
            what: "H2's live check of the discovery tool against real adapters",
        }],
    },
    Threat {
        row: "06-15",
        attack: "A silently dropped meta key leaves provenance unlabelled",
        tests: &[
            "row15_the_harness_drops_unsafe_keys_so_provenance_keys_must_be_safe",
            "gated_row15_adapter_refuses_a_partial_provenance_set",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G4,
            what: "the Claude adapter refuses to hand off when a provenance key would be dropped (gated_row15_adapter_refuses_a_partial_provenance_set)",
        }],
    },
    Threat {
        row: "06-16",
        attack: "Provenance spoofing through the body, Claude",
        tests: &[
            "row16_content_claiming_another_sender_does_not_change_provenance",
            "row16_forged_channel_tag_in_content_adds_no_attribute",
            "row16_pre_escaped_closer_in_content_adds_no_attribute",
            "row16_mid_turn_hostile_content_adds_no_attribute",
            "row16_meta_key_injection_through_content_adds_no_attribute",
            "row16_a_line_break_in_a_provenance_value_cannot_pass_the_envelope_stage",
            "gated_row16_adapter_takes_provenance_only_from_verified_members",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G4,
            what: "the G4 adapter's own meta mapping (gated_row16_adapter_takes_provenance_only_from_verified_members); the exact text of a sender-written `<\\/channel>` is a RenderGap until a capture records it",
        }],
    },
    Threat {
        row: "06-17",
        attack: "Provenance spoofing through a forged header or delimiter, Codex",
        tests: &["gated_row17_codex_frame_uses_a_receiver_generated_delimiter"],
        status: Status::Gated,
        gated: &[Gate {
            issue: G7,
            what: "the Codex adapter's frame builder (gated_row17_codex_frame_uses_a_receiver_generated_delimiter)",
        }],
    },
    Threat {
        row: "06-18",
        attack: "Reply misattribution through a forged in_reply_to",
        tests: &[
            "row02_reply_right_covers_only_the_reply_to_the_one_message",
            "gated_row18_codex_reply_correlation_is_not_trusted_alone",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G8,
            what: "the Codex adapter's reply correlation (gated_row18_codex_reply_correlation_is_not_trusted_alone)",
        }],
    },
    Threat {
        row: "06-19",
        attack: "Stale registration replay after resume",
        tests: &[
            "row19_a_registration_record_signed_by_another_device_binds_nothing",
            "row19_an_ended_session_receives_nothing",
            "gated_row19_session_lifetime_follows_the_ipc_connection",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: G9,
            what: "a session's lifetime tied to its IPC connection (gated_row19_session_lifetime_follows_the_ipc_connection)",
        }],
    },
    Threat {
        row: "06-20",
        attack: "Session-id spoofing",
        tests: &[
            "row20_claiming_a_session_id_bound_to_another_key_is_refused_with_a_finding",
            "row20_a_refused_claim_binds_nothing",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "06-21",
        attack: "Prompt injection through a memory reference",
        tests: &["row21_a_memory_reference_stays_content"],
        status: Status::Proven,
        gated: &[Gate {
            issue: L10,
            what: "L10's opt-in scenario with a real memory service (Stage 5)",
        }],
    },
    Threat {
        row: "06-22",
        attack: "False authority through a cited memory reference",
        tests: &[
            "row22_a_cited_memory_reference_is_never_provenance_or_authority",
            "row05_content_never_reaches_an_authorization_decision",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: L10,
            what: "L10's opt-in scenario with a real memory service (Stage 5)",
        }],
    },
    Threat {
        row: "06-23",
        attack: "Capture of delivered content by an external memory service",
        tests: &[],
        status: Status::OpenRisk,
        gated: &[],
    },
    Threat {
        row: "06-24",
        attack: "Session binding through a spoofed CLAUDE_CODE_SESSION_ID",
        tests: &["gated_row24_spoofed_session_variable_binds_nothing"],
        status: Status::Gated,
        gated: &[Gate {
            issue: G9,
            what: "the daemon's binding of native signals to attachments (gated_row24_spoofed_session_variable_binds_nothing)",
        }],
    },
    Threat {
        row: "S13-squatting",
        attack: "Session-id squatting by a related device, through presence",
        tests: &[
            "s13_squatting_announcement_marks_conflict_and_fails_closed",
            "s13_own_session_is_never_marked_under_conflict",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "S13-existence-oracle",
        attack: "An unauthorized sender learns whether a session exists",
        tests: &["s13_unauthorized_refusal_is_the_same_whether_or_not_the_session_exists"],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "S13-presence-forgery",
        attack: "Presence forgery, tampering, forwarding or replay",
        tests: &[
            "s13_presence_signed_by_an_unpaired_key_is_discarded",
            "s13_tampered_presence_record_is_discarded",
            "s13_forwarded_presence_record_is_discarded",
            "s13_replayed_stale_presence_record_is_discarded",
            "s13_unrelated_issuer_cannot_announce",
            "s13_forged_withdrawal_cannot_take_a_session_offline",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "S13-receipt-forgery",
        attack: "Receipt forgery, or a receipt from the wrong receiver",
        tests: &[
            "s13_receipt_from_an_unpaired_key_is_discarded",
            "s13_receipt_from_another_trusted_device_is_discarded",
            "s13_altered_receipt_is_discarded",
            "s13_receipt_for_an_envelope_never_sent_is_discarded",
            "s13_receipt_with_the_wrong_nonce_is_discarded",
            "s13_receipt_claiming_the_sender_observer_is_discarded",
            "s13_no_receipt_for_an_unverified_copy",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "S13-pairing",
        attack: "Pairing man-in-the-middle, or trust on first use",
        tests: &[
            "s13_pairing_mitm_substitution_is_caught_by_the_code",
            "s13_second_offer_ends_the_pairing",
            "s13_five_wrong_codes_abort_and_the_window_expires",
            "s13_key_id_comparison_refuses_a_substituted_key",
            "s13_a_signature_alone_never_makes_a_key_trusted",
        ],
        status: Status::Proven,
        gated: &[],
    },
    Threat {
        row: "S13-exhaustion",
        attack: "Registry and quota exhaustion",
        tests: &[
            "s13_full_duplicate_store_refuses_without_evicting",
            "s13_one_issuer_cannot_fill_the_presence_registry",
            "s13_presence_registry_capacity_is_bounded",
            "s13_receipt_allowance_is_per_device_and_bounded",
            "s13_oversized_envelope_is_refused_before_parsing",
            "gated_s13_envelope_bindings_are_bounded",
        ],
        status: Status::Proven,
        gated: &[Gate {
            issue: BINDINGS,
            what: "binding-table entries that security step 4 creates are not bounded yet (gated_s13_envelope_bindings_are_bounded)",
        }],
    },
];
