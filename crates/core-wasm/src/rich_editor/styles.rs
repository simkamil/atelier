// SPDX-License-Identifier: AGPL-3.0-or-later
//! Styles of the rich editor session (US-250). Extension module of F0: the
//! handlers of `ListStyles`, `ApplyStyle` and `PreviewStyle`. **Owner:
//! US-250** (with `web/styles-pane.mjs`). Contract:
//! `docs/us-240-format-protocol.md`.
//!
//! The preview never touches the document, its history, its revision or its
//! `modified` flag: the engine lays the touched paragraphs out through a
//! `LayoutContext` that substitutes them, and `cancel_preview` lays them out
//! again from the real document (the changed pages go to `Session::pending`,
//! reported by the next `State`).

use super::Session;
use crate::rich_protocol::{Command, ErrorCode, HitMode, Response, StyleFilter, StyleTarget};

/// The preview in progress, if any (US-250 adds what it needs to undo it).
#[derive(Default)]
pub(super) struct Preview {
    pub(super) active: bool,
}

/// True for the commands that end a preview before they run. Reading
/// commands and the layout pump keep it: the pages they send show the preview.
pub(super) fn cancels_preview(command: &Command) -> bool {
    !matches!(
        command,
        Command::PreviewStyle { .. }
            | Command::Pages { .. }
            | Command::Layout { .. }
            | Command::TextWindow { .. }
            | Command::MarkSaved { .. }
            | Command::AccessibleWindow { .. }
            | Command::SelectionRects { .. }
            | Command::ImageBitmap { .. }
            | Command::Resync
            | Command::Close
            | Command::Save { .. }
            | Command::Search { .. }
            | Command::SearchRects { .. }
            | Command::Statistics
            | Command::PropertiesRead
            | Command::FormatState
            | Command::ListStyles { .. }
            | Command::ListFonts
            | Command::FormatMarks { .. }
            | Command::HitTest {
                mode: HitMode::Probe,
                ..
            }
    )
}

/// Hook of `Session::run`: ends the preview in progress before a command that
/// cancels it. Stub: there is never a preview.
pub(super) fn cancel_preview(session: &mut Session) {
    session.preview.active = false;
}

impl Session {
    /// `ListStyles`. Stub of US-250: documented error until written.
    pub(super) fn list_styles(
        &mut self,
        _filter: StyleFilter,
        _used_only: bool,
    ) -> Result<Response, ErrorCode> {
        Err(ErrorCode::FormatNotImplemented)
    }

    /// `ApplyStyle`. Stub of US-250: documented error until written.
    pub(super) fn apply_style(
        &mut self,
        _base_revision: u64,
        _target: StyleTarget,
        _id: &str,
    ) -> Result<Response, ErrorCode> {
        self.deny_formatting()?;
        Err(ErrorCode::FormatNotImplemented)
    }

    /// `PreviewStyle`. Stub of US-250: documented error until written.
    pub(super) fn preview_style(
        &mut self,
        _target: StyleTarget,
        _show: bool,
        _id: &str,
    ) -> Result<Response, ErrorCode> {
        self.deny_formatting()?;
        Err(ErrorCode::FormatNotImplemented)
    }
}
