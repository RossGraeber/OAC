// SPDX-License-Identifier: Apache-2.0

//! The `ForgesAttachment` breach of `tests/stand_in.rs`: a connection handle the adapter
//! makes itself ([IFC-ADP-013]). It lives in the suite's library, which the static scan of a
//! stand-in is not given, so that the dynamic check is what catches it there; the static scan
//! catches the same call in `source.rs`'s own tests. (It was a `#[path]` module of
//! `stand_in.rs` until the scan learned to follow `#[path]`, #324.)

use oac_core::adapter::{Attachment, Connection};

/// A handle the core never issued.
pub fn forged_attachment() -> Attachment {
    Connection::accept(std::io::empty(), std::io::sink())
        .handle()
        .clone()
}
