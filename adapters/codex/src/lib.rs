// SPDX-License-Identifier: Apache-2.0

//! The Codex App Server adapter.
//!
//! Scaffold only (#50): no protocol logic yet. Provider-specific types stay inside this
//! crate. It depends on `oac-core` only and routes every outbound message through the core,
//! never to a transport (`spec/interfaces.md` [IFC-ADP-001]).
