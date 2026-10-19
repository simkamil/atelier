// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the style handlers (US-250). Until they are written they answer the documented error; US-250 replaces the stub test below with
//! its own tests and is the only story to edit this file.

use super::format_tests::Harness;
use crate::rich_protocol::{Command, ErrorCode, StyleFilter, StyleTarget};

#[test]
fn the_style_stubs_answer_the_documented_error_and_change_nothing() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let revision = h.state().revision;
    assert_eq!(
        h.error(Command::ListStyles {
            filter: StyleFilter::All,
            used_only: false
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: crate::rich_protocol::NO_REVISION_GUARD,
            command_id: 0,
            target: StyleTarget::Paragraph,
            id: "Heading1".into()
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: true,
            id: "Heading1".into()
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(h.state().revision, revision);
}
