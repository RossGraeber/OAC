// SPDX-License-Identifier: Apache-2.0

//! G1 reports no carrier loss. Sealed presence travels through the device subscription,
//! where the core opens and dispatches it. G2 (#63) will add native liveliness here.

use oac_core::transport::PresenceHandler;
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct Watchers {
    handlers: Mutex<Vec<PresenceHandler>>,
}
impl Watchers {
    pub(crate) fn add(&self, handler: PresenceHandler) {
        self.handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handler);
    }
}
