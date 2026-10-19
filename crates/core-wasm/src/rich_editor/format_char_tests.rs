// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the character-formatting handler (US-240). Until it is written, `ApplyCharFormat` answers the documented error; US-240 replaces
//! the stub test below with its own tests and is the only story to edit this file.

use super::format_tests::{Harness, char_format};
use crate::rich_protocol::{Command, ErrorCode};

#[test]
fn the_char_stub_answers_the_documented_error_and_changes_nothing() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let revision = h.state().revision;
    assert_eq!(
        h.error(Command::ApplyCharFormat(char_format())),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(h.state().revision, revision);
    h.type_text("!");
    assert_eq!(h.state().revision, revision + 1);
}
