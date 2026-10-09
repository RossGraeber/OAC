// SPDX-License-Identifier: Apache-2.0

//! Replies and correlation (`spec/session-channels.md` §8.2; #55, F6): which reply headers a
//! sending implementation sets, and which envelope a received reply answers. Both rely only
//! on the implementation's own records of envelopes it handed off or sent, never on a value a
//! harness or a model supplies (`docs/planning/decisions/C6-trust-rendering.md` §10).
//!
//! The records themselves are kept by whoever keeps hand-off and sent records (the
//! authorization engine of task F5 keeps both, for reply rights); this module only reads
//! them, as [`EnvelopeRecord`] values.

use crate::envelope::{Envelope, EnvelopeDraft};
use crate::ids::{SessionId, Token};

/// A hand-off record or a sent-envelope record (§8.2.2): an envelope's `id`, `from`, `to`,
/// and its `conversation_id` and `correlation_id` when present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeRecord {
    /// The envelope's `id`.
    pub id: Token,
    /// The envelope's `from`.
    pub from: SessionId,
    /// The envelope's `to`.
    pub to: SessionId,
    /// The envelope's `conversation_id`, when present.
    pub conversation_id: Option<Token>,
    /// The envelope's `correlation_id`, when present.
    pub correlation_id: Option<Token>,
}

impl EnvelopeRecord {
    /// The record of `env`.
    pub fn of(env: &Envelope) -> EnvelopeRecord {
        EnvelopeRecord {
            id: env.id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            conversation_id: env.conversation_id().cloned(),
            correlation_id: env.correlation_id().cloned(),
        }
    }
}

/// The reply headers a sending implementation sets (§8.2.2). All three are `None` for an
/// uncorrelated reply.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplyHeaders {
    /// `reply_to`.
    pub reply_to: Option<Token>,
    /// `conversation_id`, copied from the hand-off record ([SC-RCP-053]).
    pub conversation_id: Option<Token>,
    /// `correlation_id`, copied from the hand-off record ([SC-RCP-054]).
    pub correlation_id: Option<Token>,
}

impl ReplyHeaders {
    /// Whether the reply is correlated: whether `reply_to` is set. The requesting harness
    /// should be told ([SC-RCP-055]).
    pub fn correlated(&self) -> bool {
        self.reply_to.is_some()
    }

    /// `draft` with these headers set.
    pub fn apply(&self, mut draft: EnvelopeDraft) -> EnvelopeDraft {
        if let Some(v) = &self.reply_to {
            draft = draft.with_reply_to(v.clone());
        }
        if let Some(v) = &self.conversation_id {
            draft = draft.with_conversation_id(v.clone());
        }
        if let Some(v) = &self.correlation_id {
            draft = draft.with_correlation_id(v.clone());
        }
        draft
    }
}

/// The headers of a reply from `from` to `to` for the `requested_target` a harness named, if
/// any (§8.2.2). `reply_to` is set only to the requested target, and only when a hand-off
/// record shows that envelope handed off to `from`, from `to` ([SC-RCP-050] to
/// [SC-RCP-052]); otherwise the reply goes uncorrelated, and no other envelope is ever
/// guessed, even when only one candidate exists.
pub fn reply_headers<'a>(
    handed_off: impl IntoIterator<Item = &'a EnvelopeRecord>,
    from: &SessionId,
    to: &SessionId,
    requested_target: Option<&str>,
) -> ReplyHeaders {
    let Some(target) = requested_target else {
        return ReplyHeaders::default();
    };
    handed_off
        .into_iter()
        .find(|r| r.id.as_str() == target && &r.to == from && &r.from == to)
        .map(|r| ReplyHeaders {
            reply_to: Some(r.id.clone()),
            conversation_id: r.conversation_id.clone(),
            correlation_id: r.correlation_id.clone(),
        })
        .unwrap_or_default()
}

/// The envelope that `reply` answers: a sent record whose `id` is the reply's `reply_to`,
/// sent from the reply's `to` to the reply's `from` ([SC-RCP-060]). `None` for an unmatched
/// reply, which is still a valid message ([SC-RCP-061]). A matching `correlation_id` or
/// `conversation_id` alone is never a match ([SC-RCP-062]).
pub fn answered<'a>(
    sent: impl IntoIterator<Item = &'a EnvelopeRecord>,
    reply: &Envelope,
) -> Option<&'a EnvelopeRecord> {
    let target = reply.reply_to()?;
    sent.into_iter()
        .find(|r| &r.id == target && &r.from == reply.to() && &r.to == reply.from())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, from: &str, to: &str) -> EnvelopeRecord {
        EnvelopeRecord {
            id: Token::parse(id).unwrap(),
            from: SessionId::parse(from).unwrap(),
            to: SessionId::parse(to).unwrap(),
            conversation_id: Token::parse("conv"),
            correlation_id: None,
        }
    }

    const A: &str = "01harn7x9k2m4p6q8r0s2t4v6w";
    const B: &str = "7gq3m8z2c5k9t1w4x6b0n2r8vd";

    #[test]
    fn only_a_checked_target_correlates() {
        let records = [rec("m1", A, B)];
        let (a, b) = (SessionId::parse(A).unwrap(), SessionId::parse(B).unwrap());
        let h = reply_headers(&records, &b, &a, Some("m1"));
        assert!(h.correlated());
        assert_eq!(h.conversation_id, Token::parse("conv"));
        // No target, a wrong target, or the wrong direction: uncorrelated.
        assert!(!reply_headers(&records, &b, &a, None).correlated());
        assert!(!reply_headers(&records, &b, &a, Some("m2")).correlated());
        assert!(!reply_headers(&records, &a, &b, Some("m1")).correlated());
    }
}
