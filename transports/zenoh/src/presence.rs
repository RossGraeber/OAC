// SPDX-License-Identifier: Apache-2.0

//! Presence carriage: `send_presence` and `watch_presence` (`spec/interfaces.md` §6.5, §6.6;
//! [IFC-TRN-050], [IFC-TRN-060]).
//!
//! A presence record is carried like a receipt: one frame of kind `presence`, put on the
//! key expression of the device its destination names, and handed whole to the
//! `watch_presence` handlers of the peer started with that device's key. A peer of the
//! partition started with another key drops it.
//!
//! This module is kept apart from the rest of the transport so that G2 (#63) can add
//! carrier loss over liveliness tokens ([IFC-TRN-061]) here, without touching publish and
//! subscribe. G1 reports no carrier loss.

use std::sync::{Arc, Mutex};

use oac_core::transport::{CarrierHandle, Payload, PayloadKind, PresenceEvent, PresenceHandler};

use crate::gate::Gate;

/// The `watch_presence` handlers of one started transport.
#[derive(Default)]
pub(crate) struct Watchers {
    handlers: Mutex<Vec<PresenceHandler>>,
}

impl Watchers {
    pub(crate) fn add(&self, h: PresenceHandler) {
        self.handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(h);
    }

    /// Hand a received presence record, whole, to every handler, behind the transport's
    /// gate ([IFC-TRN-071]).
    pub(crate) fn record(&self, gate: &Arc<Gate>, octets: &[u8], carrier: CarrierHandle) {
        let handlers = self
            .handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let payload = Payload::new(PayloadKind::Presence, octets.to_vec());
        for h in handlers {
            let Some(_pass) = gate.enter() else { return };
            h(PresenceEvent::Record {
                payload: payload.clone(),
                carrier: carrier.clone(),
            });
        }
    }
}
