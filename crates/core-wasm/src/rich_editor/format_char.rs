// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character formatting of the rich editor session (US-240). Extension module
//! of F0: the handler of `ApplyCharFormat` and the two hooks of typing
//! attributes. **Owner: US-240** (with `web/format-char.mjs`); nothing else of
//! the engine has to change. Contract: `docs/us-240-format-protocol.md`.
//!
//! What US-240 writes here, using `RichDocument::apply_format` (one history
//! step, one revision; `crates/text-model/src/rich/edit/format.rs`):
//!
//! * `apply_char_format`: the target range (selection, else the word under the
//!   caret, else the typing attributes), the `SetCharAttrs` operations per
//!   paragraph, the revision guard, the `Toggle` and size-step rules, then
//!   `self.cache.apply_edit` and `self.finish` like `Session::edit`.
//! * `Typing`: the attributes of the next typed text.
//! * `after_text_edit`: applies the typing attributes to the text just
//!   inserted, joined with the insertion (`join_previous`).

use atelier_text_model::rich::CharAttrs;
use atelier_text_model::rich::flat::EditOutcome;

use super::Session;
use crate::rich_protocol::{CharFormat, ErrorCode, Response};

/// Attributes of the next typed text when the caret is not in a word:
/// `clear` then `set` over what the text would inherit. Forgotten at every
/// selection change (`Session::select`), kept by consecutive typing
/// (`after_text_edit`).
#[derive(Default)]
pub(super) struct Typing {
    pub(super) set: CharAttrs,
    pub(super) clear: CharAttrs,
    /// Pending: `FormatState` shows it (`typing` flag) and the next insertion applies it.
    pub(super) armed: bool,
}

impl Typing {
    /// Called by `Session::select`: any selection change drops the attributes.
    pub(super) fn forget(&mut self) {
        *self = Self::default();
    }
}

/// Hook of `Session::edit`, run after the text edit and before the selection
/// is set: applies the typing attributes to the inserted `start..start +
/// inserted` and returns the outcome the layout must be given. It cannot fail:
/// the insertion is done, so a formatting that fails leaves the text unformatted.
/// Stub: the outcome is returned unchanged.
pub(super) fn after_text_edit(
    _session: &mut Session,
    _start: usize,
    _inserted: usize,
    outcome: EditOutcome,
) -> EditOutcome {
    outcome
}

impl Session {
    /// `ApplyCharFormat`. Stub of US-240: documented error until written.
    pub(super) fn apply_char_format(&mut self, _format: CharFormat) -> Result<Response, ErrorCode> {
        self.deny_formatting()?;
        Err(ErrorCode::FormatNotImplemented)
    }
}
