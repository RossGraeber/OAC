// SPDX-License-Identifier: Apache-2.0

//! The library half of the `oac` binary: what the binary constructs and hands to the core.
//! Nothing depends on this crate (`docs/planning/v0.1/07-repository-and-dependencies.md`
//! section 3); the library target exists so its parts are built and tested before a verb
//! wires them in.
//!
//! - [`keystore`]: the device-key stores, the operating system's credential store and the
//!   encrypted-file fallback (#52, F3; `docs/planning/decisions/C4-session-identity.md`
//!   §10-§11).

pub mod keystore;
