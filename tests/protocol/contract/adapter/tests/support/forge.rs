// SPDX-License-Identifier: Apache-2.0

//! The `ForgesAttachment` breach of `stand_in.rs`: a connection handle the adapter makes
//! itself ([IFC-ADP-013]). Kept out of the sources the static scan is given, so that the
//! dynamic check is what catches it there; the static scan catches the same call in
//! `source.rs`'s own tests.

use oac_core::adapter::{Attachment, Connection};

/// A handle the core never issued.
pub fn handle() -> Attachment {
    Connection::accept(std::io::empty(), std::io::sink())
        .handle()
        .clone()
}
