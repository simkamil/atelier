// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the paragraph-formatting handler and of `FormatMarks` (US-243). Until they are written they answer the documented error; US-243 replaces
//! the stub test below with its own tests and is the only story to edit this file.

use super::format_tests::{Harness, para_format};
use crate::rich_protocol::{Command, ErrorCode};

#[test]
fn the_para_stubs_answer_the_documented_error_and_change_nothing() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let revision = h.state().revision;
    assert_eq!(
        h.error(Command::ApplyParaFormat(para_format())),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::FormatMarks { first: 0, count: 1 }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(h.state().revision, revision);
}
