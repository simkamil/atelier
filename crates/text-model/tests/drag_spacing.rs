// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_text_model::{PlainTextDocument, PlainTextError, PlainTextLimits, Selection};

fn document(text: &[u8]) -> PlainTextDocument {
    PlainTextDocument::from_utf8(text, PlainTextLimits::default()).unwrap()
}

#[test]
fn isolated_replacement_undo_redo_keeps_typing_on_separate_groups() {
    let mut doc = document(b"mot");
    doc.set_selection(Selection::caret(3)).unwrap();
    doc.insert_text(":", 10).unwrap();
    let raw_selection = doc.selection();

    doc.replace_range_isolated(3..4, "\u{00a0}:", 11).unwrap();
    assert_eq!(doc.text(), "mot\u{00a0}:");
    assert!(doc.undo().unwrap());
    assert_eq!(doc.text(), "mot:");
    assert_eq!(doc.selection(), raw_selection);
    assert!(doc.redo().unwrap());
    assert_eq!(doc.text(), "mot\u{00a0}:");

    doc.insert_text("x", 12).unwrap();
    assert!(doc.undo().unwrap());
    assert_eq!(
        doc.text(),
        "mot\u{00a0}:",
        "typing after substitution is independently undoable"
    );
    assert!(doc.undo().unwrap());
    assert_eq!(
        doc.text(),
        "mot:",
        "the next undo removes only the isolated substitution"
    );
}

#[test]
fn isolated_replacement_converts_ascii_space_and_restores_directed_selection() {
    let mut doc = document("mot : suite".as_bytes());
    doc.set_selection(Selection {
        anchor: 5,
        focus: 4,
    })
    .unwrap();
    let before = doc.selection();
    doc.replace_range_isolated(3..4, "\u{00a0}", 1).unwrap();
    assert_eq!(doc.text(), "mot\u{00a0}: suite");
    assert!(doc.undo().unwrap());
    assert_eq!(doc.text(), "mot : suite");
    assert_eq!(doc.selection(), before);
    assert!(doc.redo().unwrap());
    assert_eq!(doc.text(), "mot\u{00a0}: suite");
}

#[test]
fn move_drop_before_and_after_selection_preserves_unicode_crlf_and_bom() {
    let imported = b"\xef\xbb\xbfA\xc3\xa9\r\nBC";
    let mut before = document(imported);
    before
        .set_selection(Selection {
            anchor: 6,
            focus: 5,
        })
        .unwrap();
    before.drop_selection(0, false, 10).unwrap();
    assert_eq!(before.text(), "BAé\r\nC");
    assert_eq!(before.selection(), Selection::caret(1));
    assert!(before.undo().unwrap());
    assert_eq!(before.text(), "Aé\r\nBC");
    assert_eq!(
        before.selection(),
        Selection {
            anchor: 6,
            focus: 5
        }
    );
    assert!(before.redo().unwrap());
    assert_eq!(before.text(), "BAé\r\nC");
    assert_eq!(
        before.export_utf8().unwrap(),
        b"\xef\xbb\xbfBA\xc3\xa9\r\nC"
    );

    let mut after = document("Aé\r\nBC".as_bytes());
    after
        .set_selection(Selection {
            anchor: 5,
            focus: 6,
        })
        .unwrap();
    after.drop_selection("Aé\r\nBC".len(), false, 20).unwrap();
    assert_eq!(after.text(), "Aé\r\nCB");
    assert!(after.undo().unwrap());
    assert_eq!(after.text(), "Aé\r\nBC");
    assert_eq!(
        after.selection(),
        Selection {
            anchor: 5,
            focus: 6
        }
    );
    assert!(after.redo().unwrap());
    assert_eq!(after.text(), "Aé\r\nCB");
}

#[test]
fn move_inside_or_onto_source_is_an_exact_noop_but_copy_inside_inserts() {
    let mut moved = document(b"abcdef");
    moved
        .set_selection(Selection {
            anchor: 4,
            focus: 1,
        })
        .unwrap();
    for target in 1..=4 {
        moved.drop_selection(target, false, 1).unwrap();
        assert_eq!(moved.text(), "abcdef");
        assert_eq!(
            moved.selection(),
            Selection {
                anchor: 4,
                focus: 1
            }
        );
        assert!(!moved.can_undo());
    }

    let mut copied = document(b"abcdef");
    copied
        .set_selection(Selection {
            anchor: 1,
            focus: 4,
        })
        .unwrap();
    copied.drop_selection(2, true, 2).unwrap();
    assert_eq!(copied.text(), "abbcdcdef");
    assert_eq!(copied.selection(), Selection::caret(5));
    assert!(copied.undo().unwrap());
    assert_eq!(copied.text(), "abcdef");
    assert_eq!(
        copied.selection(),
        Selection {
            anchor: 1,
            focus: 4
        }
    );
    assert!(copied.redo().unwrap());
    assert_eq!(copied.text(), "abbcdcdef");

    let mut reversed_copy = document(b"abcdef");
    let directed = Selection {
        anchor: 4,
        focus: 1,
    };
    reversed_copy.set_selection(directed).unwrap();
    reversed_copy.drop_selection(2, true, 3).unwrap();
    assert_eq!(reversed_copy.text(), "abbcdcdef");
    assert!(reversed_copy.undo().unwrap());
    assert_eq!(reversed_copy.text(), "abcdef");
    assert_eq!(
        reversed_copy.selection(),
        directed,
        "undo restores the reversed source direction"
    );
    assert!(reversed_copy.redo().unwrap());
    assert_eq!(reversed_copy.text(), "abbcdcdef");
}

#[test]
fn invalid_offsets_and_budget_or_text_limit_fail_without_state_changes() {
    let mut unicode = document("A🦀Z".as_bytes());
    unicode
        .set_selection(Selection {
            anchor: 1,
            focus: 1,
        })
        .unwrap();
    let selection = unicode.selection();
    for range in [2..3, 0..99, core::ops::Range { start: 3, end: 2 }] {
        assert!(matches!(
            unicode.replace_range_isolated(range, "x", 1),
            Err(PlainTextError::InvalidOffset { .. } | PlainTextError::OffsetNotCharBoundary { .. })
        ));
        assert_eq!(unicode.text(), "A🦀Z");
        assert_eq!(unicode.selection(), selection);
        assert!(!unicode.can_undo());
    }
    assert!(matches!(
        unicode.drop_selection(2, true, 1),
        Err(PlainTextError::OffsetNotCharBoundary { .. })
    ));
    assert_eq!(unicode.text(), "A🦀Z");
    assert_eq!(unicode.selection(), selection);
    assert!(!unicode.can_undo());

    let tiny_history = PlainTextLimits {
        max_text_bytes: 16,
        max_history_bytes: 1,
    };
    let mut history_limited = PlainTextDocument::from_utf8(b"abc", tiny_history).unwrap();
    history_limited.set_selection(Selection::caret(1)).unwrap();
    let selection = history_limited.selection();
    assert!(matches!(
        history_limited.replace_range_isolated(1..2, "replacement", 1),
        Err(PlainTextError::HistoryBudgetExceeded { .. })
    ));
    assert_eq!(history_limited.text(), "abc");
    assert_eq!(history_limited.selection(), selection);
    assert!(!history_limited.can_undo());

    let tiny_text = PlainTextLimits {
        max_text_bytes: 3,
        max_history_bytes: 4096,
    };
    let mut text_limited = PlainTextDocument::from_utf8(b"abc", tiny_text).unwrap();
    text_limited.set_selection(Selection::caret(1)).unwrap();
    let selection = text_limited.selection();
    assert!(matches!(
        text_limited.replace_range_isolated(1..1, "x", 1),
        Err(PlainTextError::TextTooLarge { .. })
    ));
    assert_eq!(text_limited.text(), "abc");
    assert_eq!(text_limited.selection(), selection);
    assert!(!text_limited.can_undo());
}
