// SPDX-License-Identifier: AGPL-3.0-or-later
//! Paragraph formatting of the rich editor session (US-243). Extension module
//! of F0: the handler of `ApplyParaFormat`. **Owner: US-243** (with
//! `web/format-para.mjs`). Contract: `docs/us-240-format-protocol.md`.
//!
//! US-243 writes `apply_para_format` here: the paragraphs touched by the
//! selection (the one a selection ends at the very start of excepted), one
//! `SetParaProps` per paragraph, then `RichDocument::apply_format` (one history
//! step, one revision), `self.cache.apply_edit` and `self.finish` like
//! `Session::edit`. Shading and borders need model fields first (task
//! US-243-M1 of the audit in the contract).

use super::Session;
use crate::rich_protocol::{ErrorCode, ParaFormat, Response};

// `FormatMarks` (the marks of "show formatting marks") is also US-243's: the
// handler below answers the documented error until it is written. It reads
// the layout only, so it is allowed on a read-only or plain-text document.

impl Session {
    /// `FormatMarks`. Stub of US-243: documented error until written.
    pub(super) fn format_marks(&mut self, _first: u32, _count: u32) -> Result<Response, ErrorCode> {
        Err(ErrorCode::FormatNotImplemented)
    }

    /// `ApplyParaFormat`. Stub of US-243: documented error until written.
    pub(super) fn apply_para_format(&mut self, format: ParaFormat) -> Result<Response, ErrorCode> {
        self.begin_format(format.base_revision, format.command_id)?;
        Err(ErrorCode::FormatNotImplemented)
    }
}
