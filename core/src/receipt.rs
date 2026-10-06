// SPDX-License-Identifier: Apache-2.0

//! `DeliveryReceipt` (`spec/interfaces.md` §4.6): the receipt of
//! `spec/session-channels.md` §8.1.4.

use crate::delivery::{DeliveryState, ErrorCode, Observer, is_error_code_form};
use crate::ids::{Timestamp, Token};
use crate::json::{self, Json, JsonObject};

/// The `error` of a receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptError {
    /// A code of Table 8.3.
    Known(ErrorCode),
    /// A code of the right form that Table 8.3 does not list: one a later minor revision
    /// may add (§5.2 item 5). Only a received receipt holds one; [`DeliveryReceipt::new`]
    /// takes [`ErrorCode`] only, so this crate never emits one ([SC-RCP-074]).
    Unrecognized(String),
}

impl ReceiptError {
    /// The wire string.
    pub fn as_str(&self) -> &str {
        match self {
            ReceiptError::Known(c) => c.as_str(),
            ReceiptError::Unrecognized(s) => s,
        }
    }
}

/// The requirement a receipt fails, which makes a peer discard it ([SC-RCP-032]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptViolation {
    /// [SC-RCP-020]: not an I-JSON object, or a `null` inside.
    NotIJsonObject,
    /// [SC-RCP-021]: a required member is missing.
    MissingMember,
    /// [SC-RCP-022]: `envelope_id` or `envelope_from` is not an identifier token.
    NotToken,
    /// [SC-RCP-001]: `state` is not one state of Table 8.1.
    UnknownState,
    /// [SC-RCP-023]: `observer` is not `sender` or `receiver`.
    BadObserver,
    /// [SC-RCP-002]: the state is not one the observer may report.
    StateNotForObserver,
    /// [SC-RCP-024]: `observed_at` is not a timestamp.
    BadTimestamp,
    /// [SC-RCP-025]: an error-carrying state without `error`.
    MissingError,
    /// [SC-RCP-026]: `error` with a state that carries none.
    UnexpectedError,
    /// [SC-RCP-027]: `error` is not of the code form.
    BadErrorForm,
    /// [SC-RCP-028]: a Table 8.3 code that Table 8.3 does not list for this state and
    /// observer.
    ErrorNotForState,
}

/// A receipt: one delivery state for one envelope (§8.1.4). It keeps the received object
/// whole, unrecognized members included ([IFC-TYP-003]); the typed accessors cover the
/// members §8.1.4 defines ([IFC-TYP-001], [IFC-TYP-050]). Nothing in it can hold a value
/// from an envelope's `content` ([SC-RCP-031]): every member is an identifier, a state,
/// a code or a timestamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReceipt {
    envelope_id: Token,
    envelope_from: Token,
    state: DeliveryState,
    observer: Observer,
    error: Option<ReceiptError>,
    observed_at: Timestamp,
    wire: JsonObject,
}

fn str_member<'a>(o: &'a JsonObject, name: &str) -> Option<&'a str> {
    o.get(name).and_then(Json::as_str)
}

impl DeliveryReceipt {
    /// Reads a receipt from its octets, checking [SC-RCP-020] to [SC-RCP-028] in that
    /// order.
    ///
    /// # Errors
    ///
    /// The first requirement the receipt fails.
    pub fn from_octets(octets: &[u8]) -> Result<DeliveryReceipt, ReceiptViolation> {
        let v = json::parse(octets).map_err(|_| ReceiptViolation::NotIJsonObject)?;
        DeliveryReceipt::from_json(&v)
    }

    /// Reads a receipt from a parsed I-JSON value, checking [SC-RCP-020] to [SC-RCP-028]
    /// in that order.
    ///
    /// # Errors
    ///
    /// The first requirement the receipt fails.
    pub fn from_json(v: &Json) -> Result<DeliveryReceipt, ReceiptViolation> {
        use ReceiptViolation as V;
        let o = v
            .as_object()
            .filter(|_| !v.contains_null())
            .ok_or(V::NotIJsonObject)?;
        for m in [
            "envelope_id",
            "envelope_from",
            "state",
            "observer",
            "observed_at",
        ] {
            if !o.contains(m) {
                return Err(V::MissingMember);
            }
        }
        let token = |m| str_member(o, m).and_then(Token::parse).ok_or(V::NotToken);
        let (envelope_id, envelope_from) = (token("envelope_id")?, token("envelope_from")?);
        let state = str_member(o, "state")
            .and_then(DeliveryState::parse)
            .ok_or(V::UnknownState)?;
        let observer = str_member(o, "observer")
            .and_then(Observer::parse)
            .ok_or(V::BadObserver)?;
        if !state.allowed_for(observer) {
            return Err(V::StateNotForObserver);
        }
        let observed_at = str_member(o, "observed_at")
            .and_then(Timestamp::parse)
            .ok_or(V::BadTimestamp)?;
        let error = match (state.carries_error(), o.get("error")) {
            (true, None) => return Err(V::MissingError),
            (false, Some(_)) => return Err(V::UnexpectedError),
            (false, None) => None,
            (true, Some(e)) => {
                let e = e
                    .as_str()
                    .filter(|s| is_error_code_form(s))
                    .ok_or(V::BadErrorForm)?;
                Some(match ErrorCode::parse(e) {
                    Some(code) if !code.fits_receipt(state, observer) => {
                        return Err(V::ErrorNotForState);
                    }
                    Some(code) => ReceiptError::Known(code),
                    None => ReceiptError::Unrecognized(e.to_owned()),
                })
            }
        };
        Ok(DeliveryReceipt {
            envelope_id,
            envelope_from,
            state,
            observer,
            error,
            observed_at,
            wire: o.clone(),
        })
    }

    /// A receipt this implementation reports. `error` must be given exactly when `state`
    /// carries one, and must be a code Table 8.3 lists for `state` and `observer`
    /// ([SC-RCP-002], [SC-RCP-025] to [SC-RCP-028]).
    ///
    /// # Errors
    ///
    /// The requirement the combination breaks.
    pub fn new(
        envelope_id: Token,
        envelope_from: Token,
        state: DeliveryState,
        observer: Observer,
        error: Option<ErrorCode>,
        observed_at: Timestamp,
    ) -> Result<DeliveryReceipt, ReceiptViolation> {
        if !state.allowed_for(observer) {
            return Err(ReceiptViolation::StateNotForObserver);
        }
        match (state.carries_error(), error) {
            (true, None) => return Err(ReceiptViolation::MissingError),
            (false, Some(_)) => return Err(ReceiptViolation::UnexpectedError),
            (true, Some(c)) if !c.fits_receipt(state, observer) => {
                return Err(ReceiptViolation::ErrorNotForState);
            }
            _ => {}
        }
        let mut wire = JsonObject::new();
        wire.insert("envelope_id", envelope_id.as_str().into());
        wire.insert("envelope_from", envelope_from.as_str().into());
        wire.insert("state", state.as_str().into());
        wire.insert("observer", observer.as_str().into());
        if let Some(c) = error {
            wire.insert("error", c.as_str().into());
        }
        wire.insert("observed_at", observed_at.as_str().into());
        Ok(DeliveryReceipt {
            envelope_id,
            envelope_from,
            state,
            observer,
            error: error.map(ReceiptError::Known),
            observed_at,
            wire,
        })
    }

    /// The envelope's `id`.
    pub fn envelope_id(&self) -> &Token {
        &self.envelope_id
    }

    /// The envelope's `from`.
    pub fn envelope_from(&self) -> &Token {
        &self.envelope_from
    }

    /// The state as carried.
    pub fn state(&self) -> DeliveryState {
        self.state
    }

    /// The observer.
    pub fn observer(&self) -> Observer {
        self.observer
    }

    /// The error, when the state carries one.
    pub fn error(&self) -> Option<&ReceiptError> {
        self.error.as_ref()
    }

    /// The observer's clock reading.
    pub fn observed_at(&self) -> &Timestamp {
        &self.observed_at
    }

    /// The state a peer processes the receipt as: `failed` when `error` is not a Table 8.3
    /// code and the state is not `duplicate`, the carried state otherwise ([SC-RCP-030]).
    pub fn effective_state(&self) -> DeliveryState {
        match (&self.error, self.state) {
            (Some(ReceiptError::Unrecognized(_)), s) if s != DeliveryState::Duplicate => {
                DeliveryState::Failed
            }
            (_, s) => s,
        }
    }

    /// The receipt as a JSON object: every member as received, or as built.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }

    /// The receipt as compact JSON octets.
    pub fn to_octets(&self) -> Vec<u8> {
        Json::Object(self.wire.clone()).to_compact().into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(s: &str) -> Token {
        Token::parse(s).unwrap()
    }

    #[test]
    fn builds_only_valid_receipts() {
        let at = Timestamp::parse("2026-10-03T12:00:01.250Z").unwrap();
        let r = DeliveryReceipt::new(
            tok("m1"),
            tok("01harn7x9k2m4p6q8r0s2t4v6w"),
            DeliveryState::Rejected,
            Observer::Receiver,
            Some(ErrorCode::Unauthorized),
            at.clone(),
        )
        .unwrap();
        let back = DeliveryReceipt::from_octets(&r.to_octets()).unwrap();
        assert_eq!(back, r);
        assert_eq!(
            DeliveryReceipt::new(
                tok("m1"),
                tok("f"),
                DeliveryState::HandedToHarness,
                Observer::Sender,
                None,
                at.clone()
            ),
            Err(ReceiptViolation::StateNotForObserver)
        );
        assert_eq!(
            DeliveryReceipt::new(
                tok("m1"),
                tok("f"),
                DeliveryState::Failed,
                Observer::Receiver,
                Some(ErrorCode::TransportFailure),
                at
            ),
            Err(ReceiptViolation::ErrorNotForState)
        );
    }
}
