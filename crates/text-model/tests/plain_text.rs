// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_text_model::{PlainTextDocument, PlainTextError, PlainTextLimits, Selection};

fn doc(text: &[u8]) -> PlainTextDocument {
    PlainTextDocument::from_utf8(text, PlainTextLimits::default()).unwrap()
}

#[test]
fn imports_exports_bom_and_line_endings_exactly() {
    let bytes = b"\xef\xbb\xbfA\r\nB\n";
    let document = doc(bytes);
    assert_eq!(document.text(), "A\r\nB\n");
    assert_eq!(document.export_utf8().unwrap(), bytes);
}

#[test]
fn invalid_utf8_and_invalid_offsets_are_rejected_without_changes() {
    assert!(matches!(
        PlainTextDocument::from_utf8(b"\xff", PlainTextLimits::default()),
        Err(PlainTextError::InvalidUtf8)
    ));
    let mut document = doc("é".as_bytes());
    let before = document.export_utf8().unwrap();
    assert!(matches!(
        document.set_selection(Selection::caret(1)),
        Err(PlainTextError::OffsetNotCharBoundary { .. })
    ));
    assert_eq!(document.export_utf8().unwrap(), before);
    assert!(!document.can_undo());
}

#[test]
fn reversed_selection_replace_undo_and_redo_restore_text_and_selection() {
    let mut document = doc(b"hello world");
    document
        .set_selection(Selection {
            anchor: 11,
            focus: 6,
        })
        .unwrap();
    document.insert_text("Atelier", 100).unwrap();
    assert_eq!(document.text(), "hello Atelier");
    assert_eq!(document.selection(), Selection::caret(13));
    assert!(document.undo().unwrap());
    assert_eq!(document.text(), "hello world");
    assert_eq!(
        document.selection(),
        Selection {
            anchor: 11,
            focus: 6
        }
    );
    assert!(document.redo().unwrap());
    assert_eq!(document.text(), "hello Atelier");
    assert_eq!(document.selection(), Selection::caret(13));
}

#[test]
fn caret_delete_removes_whole_combining_emoji_and_crlf_clusters() {
    let mut document = doc("a\u{301}👩‍👩‍👧‍👦\r\nz".as_bytes());
    document
        .set_selection(Selection::caret("a\u{301}👩‍👩‍👧‍👦\r\n".len()))
        .unwrap();
    assert!(document.delete_backward(0).unwrap());
    assert_eq!(document.text(), "a\u{301}👩‍👩‍👧‍👦z");
    assert!(document.delete_backward(1).unwrap());
    assert_eq!(document.text(), "a\u{301}z");
    assert!(document.delete_backward(2).unwrap());
    assert_eq!(document.text(), "z");
}

#[test]
fn caret_inside_crlf_still_deletes_the_entire_cluster() {
    let mut document = doc(b"a\r\nz");
    document.set_selection(Selection::caret(2)).unwrap();
    assert!(document.delete_backward(0).unwrap());
    assert_eq!(document.text(), "az");
}

#[test]
fn budget_failures_and_document_limit_leave_state_unchanged() {
    let limits = PlainTextLimits {
        max_text_bytes: 4,
        max_history_bytes: 1,
    };
    let mut document = PlainTextDocument::from_utf8(b"abc", limits).unwrap();
    document.set_selection(Selection::caret(1)).unwrap();
    let before_selection = document.selection();
    assert!(matches!(
        document.insert_text("x", 0),
        Err(PlainTextError::HistoryBudgetExceeded { .. })
    ));
    assert_eq!(document.text(), "abc");
    assert_eq!(document.selection(), before_selection);
    assert!(!document.can_undo());
    let limits = PlainTextLimits {
        max_text_bytes: 3,
        max_history_bytes: 4096,
    };
    let mut document = PlainTextDocument::from_utf8(b"abc", limits).unwrap();
    document.set_selection(Selection::caret(3)).unwrap();
    assert!(matches!(
        document.insert_text("x", 0),
        Err(PlainTextError::TextTooLarge { .. })
    ));
    assert_eq!(document.text(), "abc");
    assert!(!document.can_undo());
}

#[test]
fn close_typing_group_with_cursor_move_and_new_edit_discards_redo() {
    let mut document = doc(b"");
    document.insert_text("a", 10).unwrap();
    document.insert_text("b", 20).unwrap();
    assert!(document.undo().unwrap());
    assert_eq!(document.text(), "");
    assert!(document.can_redo());
    document.set_selection(Selection::caret(0)).unwrap();
    document.insert_text("x", 30).unwrap();
    assert!(!document.can_redo());
    assert_eq!(document.text(), "x");
}

#[test]
fn deterministic_edit_sequence_matches_oracle_through_undo_and_redo() {
    let mut document = doc(b"seed");
    let mut oracle = String::from("seed");
    let mut states = vec![oracle.clone()];
    let mut seed = 0x1234_5678_u64;
    for index in 0..120_u64 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let offset = (seed as usize) % (oracle.len() + 1);
        // Keep generated locations at UTF-8 boundaries; this oracle uses ASCII only.
        let text = char::from(b'a' + (seed as u8 % 26));
        document.set_selection(Selection::caret(offset)).unwrap();
        document.insert_text(&text.to_string(), index * 2).unwrap();
        oracle.insert(offset, text);
        states.push(oracle.clone());
        assert_eq!(document.text(), oracle);
    }
    for expected in states[..states.len() - 1].iter().rev() {
        assert!(document.undo().unwrap());
        assert_eq!(document.text(), expected);
    }
    for expected in states.iter().skip(1) {
        assert!(document.redo().unwrap());
        assert_eq!(document.text(), expected);
    }
}

#[test]
fn empty_insert_is_a_noop_with_zero_budget_and_preserves_redo() {
    let limits = PlainTextLimits {
        max_text_bytes: 8,
        max_history_bytes: 0,
    };
    let mut empty = PlainTextDocument::from_utf8(b"", limits).unwrap();
    let before = empty.selection();
    empty.insert_text("", 0).unwrap();
    assert_eq!(empty.text(), "");
    assert_eq!(empty.selection(), before);
    assert!(!empty.can_undo());

    let mut document = doc(b"");
    document.insert_text("a", 10).unwrap();
    assert!(document.undo().unwrap());
    let before = document.selection();
    document.insert_text("", 11).unwrap();
    assert_eq!(document.text(), "");
    assert_eq!(document.selection(), before);
    assert!(document.can_redo());
    assert!(document.redo().unwrap());
    assert_eq!(document.text(), "a");
}

#[test]
fn delete_forward_and_empty_document_edges_are_safe() {
    let mut empty = doc(b"");
    assert!(!empty.delete_forward(0).unwrap());
    assert!(!empty.delete_backward(0).unwrap());
    assert!(!empty.can_undo());

    let mut document = doc("🦀e\u{301}z".as_bytes());
    assert!(document.delete_forward(0).unwrap());
    assert_eq!(document.text(), "e\u{301}z");
    assert!(document.undo().unwrap());
    assert_eq!(document.text(), "🦀e\u{301}z");
    document
        .set_selection(Selection::caret(document.text().len()))
        .unwrap();
    assert!(!document.delete_forward(1).unwrap());
    assert!(document.delete_backward(2).unwrap());
    assert_eq!(document.text(), "🦀e\u{301}");
}

#[test]
fn failed_edit_keeps_an_existing_redo_branch() {
    let limits = PlainTextLimits {
        max_text_bytes: 32,
        max_history_bytes: 485,
    };
    let mut document = PlainTextDocument::from_utf8(b"", limits).unwrap();
    document.insert_text("a", 0).unwrap();
    assert!(document.undo().unwrap());
    assert_eq!(document.text(), "");
    assert!(document.can_redo());
    let selection = document.selection();
    assert!(matches!(
        document.insert_text("abcdefghij", 2),
        Err(PlainTextError::HistoryBudgetExceeded { .. })
    ));
    assert_eq!(document.text(), "");
    assert_eq!(document.selection(), selection);
    assert!(document.can_redo());
    assert!(document.redo().unwrap());
    assert_eq!(document.text(), "a");
}

#[test]
fn unicode_selection_and_deletion_sequence_matches_oracle() {
    let mut oracle = String::from("Aé🦀e\u{301}Z");
    let mut document = doc(oracle.as_bytes());
    let mut snapshots = vec![oracle.clone()];
    let mut seed = 0xa551_2026_u64;
    for step in 0..60_u64 {
        seed = seed
            .wrapping_mul(2_862_933_555_777_941_757)
            .wrapping_add(3_037_000_493);
        let boundaries: Vec<usize> = oracle
            .char_indices()
            .map(|(offset, _)| offset)
            .chain([oracle.len()])
            .collect();
        let a = seed as usize % boundaries.len();
        seed = seed.rotate_left(17).wrapping_add(0x9e37_79b9);
        let b = seed as usize % boundaries.len();
        let start = boundaries[a.min(b)];
        let end = boundaries[a.max(b)];
        let reverse = step % 2 == 1;
        document
            .set_selection(if reverse {
                Selection {
                    anchor: end,
                    focus: start,
                }
            } else {
                Selection {
                    anchor: start,
                    focus: end,
                }
            })
            .unwrap();
        let replacement = if step % 3 == 0 { "" } else { "λ🦊" };
        document.insert_text(replacement, step * 2).unwrap();
        if start == end && replacement.is_empty() {
            assert_eq!(document.text(), oracle);
            continue;
        }
        oracle.replace_range(start..end, replacement);
        snapshots.push(oracle.clone());
        assert_eq!(document.text(), oracle);
    }
    for expected in snapshots[..snapshots.len() - 1].iter().rev() {
        assert!(document.undo().unwrap());
        assert_eq!(document.text(), expected);
    }
    for expected in snapshots.iter().skip(1) {
        assert!(document.redo().unwrap());
        assert_eq!(document.text(), expected);
    }
}
