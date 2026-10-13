// SPDX-License-Identifier: AGPL-3.0-or-later
//! Readers for legacy Microsoft Office binary formats.
//!
//! [`doc`] opens Word 97-2003 documents (US-312). Spreadsheet and presentation
//! readers will be added as sibling modules. [`detect`] recognises protected
//! documents without importing them (US-315).

#![forbid(unsafe_code)]

pub mod detect;
pub mod doc;
