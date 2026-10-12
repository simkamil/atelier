// SPDX-License-Identifier: AGPL-3.0-or-later
//! OpenDocument import. The `odt` module reads text documents (`.odt`, `.ott`
//! and flat `.fodt`) into the rich text model; spreadsheets and presentations
//! will be added as sibling modules.

#![forbid(unsafe_code)]

pub mod odt;
