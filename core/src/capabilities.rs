// SPDX-License-Identifier: Apache-2.0

//! `SessionCapabilities` and `SessionDescriptor` (`spec/interfaces.md` §4.3, §4.4): the
//! capability declaration of `spec/session-channels.md` §6.4, version agreement (§6.5),
//! and the session descriptor (§6.3).

use crate::ids::{SessionId, Token, Version, is_core_type, is_extension_type};
use crate::json::{Json, JsonNumber, JsonObject, is_ijson_string};

/// The default and smallest `max_envelope_octets` ([SC-ENV-004], [SC-ID-066]).
pub const DEFAULT_MAX_ENVELOPE_OCTETS: u64 = 65_536;

/// The largest `max_envelope_octets` ([SC-ID-066]).
pub const MAX_ENVELOPE_OCTETS_LIMIT: u64 = 9_007_199_254_740_991;

/// One extension identifier an implementation implements, with its major version and the
/// revision implemented for it (§5.1; the "implemented list" of §6.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Implemented {
    /// The extension identifier.
    pub extension: String,
    /// The major version §5.1 assigns to it.
    pub major: u16,
    /// The revision implemented for it.
    pub revision: Version,
}

/// A valid capabilities entry (§6.4). Its typed members are those §6.4 defines; an
/// unrecognized member is kept in the wire object, never exposed ([SC-ID-067]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitiesEntry {
    revision: Version,
    active_inbound: bool,
    content_types: Option<Vec<String>>,
    max_envelope_octets: Option<u64>,
    wire: JsonObject,
}

impl CapabilitiesEntry {
    /// An entry with `revision` and `active_inbound`.
    pub fn new(revision: Version, active_inbound: bool) -> CapabilitiesEntry {
        let mut wire = JsonObject::new();
        wire.insert("revision", revision.to_string().as_str().into());
        wire.insert("active_inbound", Json::Bool(active_inbound));
        CapabilitiesEntry {
            revision,
            active_inbound,
            content_types: None,
            max_envelope_octets: None,
            wire,
        }
    }

    /// The entry with `content_types` ([SC-ID-063], [SC-ID-064]). `None` when a type is
    /// neither a core type nor an extension type.
    pub fn with_content_types(mut self, types: Vec<String>) -> Option<CapabilitiesEntry> {
        if !types
            .iter()
            .all(|t| is_core_type(t) || is_extension_type(t))
        {
            return None;
        }
        self.wire.insert(
            "content_types",
            Json::Array(types.iter().map(|t| t.as_str().into()).collect()),
        );
        self.content_types = Some(types);
        Some(self)
    }

    /// The entry with `max_envelope_octets` ([SC-ID-065], [SC-ID-066]). `None` outside
    /// 65536 to 9007199254740991.
    pub fn with_max_envelope_octets(mut self, octets: u64) -> Option<CapabilitiesEntry> {
        if !(DEFAULT_MAX_ENVELOPE_OCTETS..=MAX_ENVELOPE_OCTETS_LIMIT).contains(&octets) {
            return None;
        }
        self.wire.insert(
            "max_envelope_octets",
            Json::Number(JsonNumber::from_u64(octets)),
        );
        self.max_envelope_octets = Some(octets);
        Some(self)
    }

    /// Reads an entry for an identifier whose major version is `major`. `None` when it
    /// fails any of [SC-ID-061], [SC-ID-062], [SC-ID-064] and [SC-ID-066], which makes it
    /// absent ([SC-ID-068]).
    pub fn from_json(v: &Json, major: u16) -> Option<CapabilitiesEntry> {
        let o = v.as_object()?;
        let revision = o
            .get("revision")
            .and_then(Json::as_str)
            .and_then(Version::parse)
            .filter(|r| r.major == major)?;
        let active_inbound = o.get("active_inbound").and_then(Json::as_bool)?;
        let content_types = match o.get("content_types") {
            None => None,
            Some(t) => {
                let items = t.as_array()?;
                let types: Option<Vec<String>> = items
                    .iter()
                    .map(|t| {
                        t.as_str()
                            .filter(|t| is_core_type(t) || is_extension_type(t))
                            .map(str::to_owned)
                    })
                    .collect();
                Some(types?)
            }
        };
        let max_envelope_octets = match o.get("max_envelope_octets") {
            None => None,
            Some(n) => Some(
                n.as_number()?
                    .plain_integer_in(DEFAULT_MAX_ENVELOPE_OCTETS, MAX_ENVELOPE_OCTETS_LIMIT)?,
            ),
        };
        Some(CapabilitiesEntry {
            revision,
            active_inbound,
            content_types,
            max_envelope_octets,
            wire: o.clone(),
        })
    }

    /// The revision the declarer implements for the identifier.
    pub fn revision(&self) -> Version {
        self.revision
    }

    /// Whether the session accepts envelopes by active delivery (§7.1).
    pub fn active_inbound(&self) -> bool {
        self.active_inbound
    }

    /// Whether the session accepts content parts of type `t`: `text` always, others only
    /// when listed ([SC-ID-063]).
    pub fn accepts_part_type(&self, t: &str) -> bool {
        t == "text"
            || self
                .content_types
                .as_ref()
                .is_some_and(|l| l.iter().any(|x| x == t))
    }

    /// The listed part types, as declared.
    pub fn content_types(&self) -> Option<&[String]> {
        self.content_types.as_deref()
    }

    /// The largest serialized envelope the session accepts: the declared value, or the
    /// default of [SC-ENV-004] ([SC-ID-065]).
    pub fn max_envelope_octets(&self) -> u64 {
        self.max_envelope_octets
            .unwrap_or(DEFAULT_MAX_ENVELOPE_OCTETS)
    }

    /// The entry as a JSON object.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }
}

/// The version a sender and a session agree (§6.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agreed {
    /// The agreed extension identifier.
    pub extension: String,
    /// Its major version.
    pub major: u16,
    /// The revision the sender implements for it, which the envelope's `version` carries
    /// ([SC-ID-087]).
    pub revision: Version,
    /// The session's entry for it.
    pub entry: CapabilitiesEntry,
}

/// A capability declaration (§6.4): a map from extension identifier to capabilities
/// entry ([IFC-TYP-030]). It keeps the value as received, so a declaration that is not an
/// object, or that holds invalid or unknown entries, is kept unchanged and read as the
/// rules say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCapabilities {
    wire: Json,
}

impl SessionCapabilities {
    /// A declaration as received. Any JSON value is accepted: one that is not an object
    /// holds no entries ([SC-ID-070]).
    pub fn from_json(v: &Json) -> SessionCapabilities {
        SessionCapabilities { wire: v.clone() }
    }

    /// A declaration of this implementation's own: one entry per extension identifier
    /// ([SC-ID-080], [SC-ID-081]). A later entry for the same identifier replaces an
    /// earlier one.
    ///
    /// `None` when an identifier does not have the `{reverse-dns-prefix}/{name}` form of
    /// §5.1 (checked with the extension-type grammar of §4.5.2, which shares it) or is not
    /// an I-JSON string ([SC-ENV-002] read for the declaration): a declaration a peer
    /// could not read is never built.
    pub fn declare<'a>(
        entries: impl IntoIterator<Item = (&'a str, CapabilitiesEntry)>,
    ) -> Option<SessionCapabilities> {
        let mut o = JsonObject::new();
        for (ext, e) in entries {
            if !is_extension_type(ext) || !is_ijson_string(ext) {
                return None;
            }
            o.insert(ext, Json::Object(e.wire));
        }
        Some(SessionCapabilities {
            wire: Json::Object(o),
        })
    }

    /// The valid entries for the identifiers in `implemented`. A member named by any other
    /// identifier is ignored ([SC-ID-082]); an invalid entry is absent ([SC-ID-068]) and
    /// does not affect the others ([SC-ID-069]).
    pub fn valid_entries(&self, implemented: &[Implemented]) -> Vec<Agreed> {
        let Some(o) = self.wire.as_object() else {
            return Vec::new(); // [SC-ID-070]
        };
        implemented
            .iter()
            .filter_map(|imp| {
                let entry = CapabilitiesEntry::from_json(o.get(&imp.extension)?, imp.major)?;
                Some(Agreed {
                    extension: imp.extension.clone(),
                    major: imp.major,
                    revision: imp.revision,
                    entry,
                })
            })
            .collect()
    }

    /// The agreed version: the highest major version that `implemented` holds and the
    /// declaration has a valid entry for ([SC-ID-083]). `None` when there is none, and then
    /// nothing may be sent to the session ([SC-ID-084]). Minor versions play no part
    /// ([SC-ID-085]).
    pub fn agree(&self, implemented: &[Implemented]) -> Option<Agreed> {
        self.valid_entries(implemented)
            .into_iter()
            .max_by_key(|a| a.major)
    }

    /// The declaration as held.
    pub fn as_json(&self) -> &Json {
        &self.wire
    }
}

/// A session descriptor (§6.3; `spec/interfaces.md` §4.3). It has no member for a working
/// directory, a harness-native id, a cross-check value, a display form or a registration
/// time ([IFC-TYP-020]); an unrecognized member of a received descriptor is kept in the
/// wire object and never exposed ([SC-ID-044]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDescriptor {
    session_id: SessionId,
    capabilities: SessionCapabilities,
    wire: JsonObject,
}

impl SessionDescriptor {
    /// A descriptor of this implementation's own. `display_name` and `harness_label` are
    /// optional ([SC-ID-042]).
    ///
    /// `None` when `capabilities` is not a JSON object ([SC-ID-060]) or `display_name`
    /// holds a noncharacter, which I-JSON forbids: an announcement carrying the descriptor
    /// would then be discarded by every consumer ([SC-DLV-021], [SC-DLV-040]). So the
    /// implementation's own descriptor is always one a peer accepts.
    pub fn new(
        session_id: SessionId,
        capabilities: SessionCapabilities,
        display_name: Option<&str>,
        harness_label: Option<&Token>,
    ) -> Option<SessionDescriptor> {
        if capabilities.wire.as_object().is_none() || !display_name.is_none_or(is_ijson_string) {
            return None;
        }
        let mut wire = JsonObject::new();
        wire.insert("session_id", session_id.as_str().into());
        wire.insert("capabilities", capabilities.wire.clone());
        if let Some(d) = display_name {
            wire.insert("display_name", d.into());
        }
        if let Some(h) = harness_label {
            wire.insert("harness_label", h.as_str().into());
        }
        Some(SessionDescriptor {
            session_id,
            capabilities,
            wire,
        })
    }

    /// Reads a descriptor: an object with a session id in `session_id` ([SC-ID-040]) and a
    /// `capabilities` member ([SC-ID-041]; a value that is not an object holds no entries,
    /// [SC-DLV-029], [SC-ID-070]). `None` otherwise.
    pub fn from_json(v: &Json) -> Option<SessionDescriptor> {
        let o = v.as_object()?;
        let session_id = o
            .get("session_id")
            .and_then(Json::as_str)
            .and_then(SessionId::parse)?;
        let capabilities = SessionCapabilities::from_json(o.get("capabilities")?);
        Some(SessionDescriptor {
            session_id,
            capabilities,
            wire: o.clone(),
        })
    }

    /// The session's id, its only authoritative address.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// The session's capability declaration.
    pub fn capabilities(&self) -> &SessionCapabilities {
        &self.capabilities
    }

    /// `display_name`, when it is a string. For display only: never for routing,
    /// authorization or provenance ([SC-ID-043]).
    pub fn display_name(&self) -> Option<&str> {
        self.wire.get("display_name").and_then(Json::as_str)
    }

    /// `harness_label`, when it is an identifier token. For display only ([SC-ID-043]).
    pub fn harness_label(&self) -> Option<Token> {
        self.wire
            .get("harness_label")
            .and_then(Json::as_str)
            .and_then(Token::parse)
    }

    /// The descriptor as a JSON object.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::EXTENSION_ID_V0;
    use crate::json::parse;

    fn implemented() -> Vec<Implemented> {
        vec![Implemented {
            extension: EXTENSION_ID_V0.into(),
            major: 0,
            revision: Version { major: 0, minor: 1 },
        }]
    }

    #[test]
    fn own_declaration_round_trips() {
        let e = CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true)
            .with_max_envelope_octets(131_072)
            .unwrap();
        let caps = SessionCapabilities::declare([(EXTENSION_ID_V0, e)]).unwrap();
        let back =
            SessionCapabilities::from_json(&parse(caps.as_json().to_compact().as_bytes()).unwrap());
        let agreed = back.agree(&implemented()).unwrap();
        assert_eq!(agreed.major, 0);
        assert_eq!(agreed.entry.max_envelope_octets(), 131_072);
        assert!(agreed.entry.accepts_part_type("text"));
        assert!(!agreed.entry.accepts_part_type("com.example/x"));
    }

    #[test]
    fn builders_refuse_out_of_range_values() {
        let e = CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true);
        assert!(e.clone().with_max_envelope_octets(65_535).is_none());
        assert!(e.with_content_types(vec!["Text".into()]).is_none());
    }

    #[test]
    fn own_declaration_and_descriptor_are_always_valid_for_peers() {
        let e = || CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true);
        // Extension identifiers: the {reverse-dns-prefix}/{name} form, I-JSON only.
        for bad in [
            "oac-session-channels",
            "io.github.rossgraeber/oac\u{fffe}",
            "io.github.rossgraeber/",
            "",
        ] {
            assert!(
                SessionCapabilities::declare([(bad, e())]).is_none(),
                "{bad:?}"
            );
        }
        let caps = SessionCapabilities::declare([(EXTENSION_ID_V0, e())]).unwrap();
        let sid = SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap();
        // A display name with a noncharacter would make the announcement unreadable.
        for bad in ["bad\u{ffff}", "\u{fdd0}", "x\u{10fffe}"] {
            assert!(SessionDescriptor::new(sid.clone(), caps.clone(), Some(bad), None).is_none());
        }
        // A declaration that is not an object (SC-ID-060).
        let not_object = SessionCapabilities::from_json(&Json::from("x"));
        assert!(SessionDescriptor::new(sid.clone(), not_object, None, None).is_none());
        let label = Token::parse("harness-b").unwrap();
        let d = SessionDescriptor::new(sid, caps, Some("Review \u{1F600}"), Some(&label)).unwrap();
        let back = SessionDescriptor::from_json(
            &parse(Json::Object(d.as_json().clone()).to_compact().as_bytes()).unwrap(),
        )
        .unwrap();
        assert_eq!(back, d);
        assert_eq!(back.display_name(), Some("Review \u{1F600}"));
        assert_eq!(back.harness_label(), Some(label));
    }
}
