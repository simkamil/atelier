// SPDX-License-Identifier: AGPL-3.0-or-later
//! Search, replace, statistics and properties of the rich editor session
//! (R7b-2), every command through the real v2 codec.

use super::*;
use crate::rich_protocol::{
    HighlightRect, PropertiesSnapshot, PropertiesStatus, Request, SaveFormat, SearchFlags,
    SearchItem, Stamp,
};
use atelier_fmt_ooxml::Package;
use atelier_model::Transaction;
use atelier_text_model::rich::{AnchoredObject, BreakKind, Inline, InlineBreak, RichOp};

struct Find {
    editor: RichEditor,
    request: u32,
}

impl Find {
    fn new() -> Self {
        Self {
            editor: RichEditor::default(),
            request: 0,
        }
    }

    fn send(&mut self, command: Command) -> Response {
        self.request += 1;
        let frame = proto::encode_request(&Request {
            doc_id: 1,
            request_id: self.request,
            command,
        })
        .unwrap();
        let reply = self.editor.respond(&Message {
            kind: MessageType::EditorResult,
            request_id: 9,
            payload: frame,
        });
        assert_eq!(reply.kind, MessageType::EditorResult);
        proto::decode_reply(&reply.payload).unwrap().response
    }

    fn state(&mut self, command: Command) -> State {
        match self.send(command) {
            Response::State(s) => s,
            other => panic!("expected a state, got {other:?}"),
        }
    }

    fn error(&mut self, command: Command) -> ErrorCode {
        match self.send(command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn open(&mut self, kind: OpenKind, profile: Profile, data: &[u8]) -> State {
        self.state(Command::Open {
            kind,
            profile,
            data: data.to_vec(),
        })
    }

    fn txt(&mut self, text: &str) -> State {
        self.open(OpenKind::Txt, Profile::Txt, text.as_bytes())
    }

    fn docx(&mut self, name: &str) -> State {
        let path = format!(
            "{}/../../tests/docx-reference/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        self.open(OpenKind::Docx, Profile::Document, &bytes)
    }

    fn with(doc: RichDocument) -> Self {
        let mut f = Self::new();
        f.editor.active =
            Some(Session::with_document(1, doc, true, io::Origin::new(io::Source::New)).unwrap());
        f
    }

    fn text(&mut self) -> String {
        match self.send(Command::TextWindow {
            start: 0,
            len: proto::MAX_WINDOW_BYTES as u32,
        }) {
            Response::TextWindow { text, .. } => text,
            other => panic!("{other:?}"),
        }
    }

    fn revision(&mut self) -> u64 {
        self.state(Command::Layout { budget: 1 }).revision
    }

    fn select(&mut self, anchor: u64, focus: u64) -> State {
        self.state(Command::SetSelection {
            anchor,
            focus,
            upstream: false,
        })
    }

    fn search(&mut self, query: &str, flags: SearchFlags) -> (u32, bool, Vec<SearchItem>) {
        let revision = self.revision();
        match self.send(Command::Search {
            revision,
            flags,
            query: query.into(),
        }) {
            Response::SearchResult {
                revision: r,
                total,
                truncated,
                items,
                ..
            } => {
                assert_eq!(r, revision);
                (total, truncated, items)
            }
            other => panic!("expected a search result, got {other:?}"),
        }
    }

    fn found(&mut self, query: &str, flags: SearchFlags) -> Vec<(u64, u64)> {
        self.search(query, flags)
            .2
            .iter()
            .map(|i| (i.start, i.end))
            .collect()
    }

    fn replace_all(&mut self, query: &str, replacement: &str) -> (u32, State) {
        let revision = self.revision();
        match self.send(Command::ReplaceAll {
            revision,
            flags: SearchFlags::default(),
            query: query.into(),
            replacement: replacement.into(),
        }) {
            Response::Replaced { count, state } => (count, state),
            other => panic!("expected Replaced, got {other:?}"),
        }
    }

    fn properties(&mut self) -> PropertiesSnapshot {
        match self.send(Command::PropertiesRead) {
            Response::Properties { snapshot, .. } => *snapshot,
            other => panic!("expected properties, got {other:?}"),
        }
    }

    fn apply(&mut self, field: usize, value: &str) -> proto::PropertiesOutcome {
        let mut changes: [Option<String>; proto::PROPERTY_COUNT] = Default::default();
        changes[field] = Some(value.into());
        match self.send(Command::PropertiesApply { changes }) {
            Response::PropertiesApplied(outcome) => outcome,
            other => panic!("expected PropertiesApplied, got {other:?}"),
        }
    }

    fn save(&mut self, format: SaveFormat) -> Vec<u8> {
        match self.send(Command::Save {
            format,
            stamp: Stamp {
                now_ms: 1_700_000_000_000,
                user: "Alice".into(),
                version: "1.2".into(),
            },
        }) {
            Response::SaveData { bytes, .. } => bytes,
            other => panic!("expected SaveData, got {other:?}"),
        }
    }
}

fn flags(
    match_case: bool,
    whole_word: bool,
    ignore_accents: bool,
    match_nbsp: bool,
) -> SearchFlags {
    SearchFlags {
        match_case,
        whole_word,
        ignore_accents,
        match_nbsp,
    }
}

/// Every occurrence must read back as the query in the flat text.
fn check_items_read_back(f: &mut Find, query: &str, items: &[(u64, u64)]) {
    let text = f.text();
    for &(start, end) in items {
        let part = &text[start as usize..end as usize];
        assert_eq!(part.to_lowercase(), query.to_lowercase(), "{start}..{end}");
    }
}

#[test]
fn search_options_follow_the_plain_editor() {
    let mut f = Find::new();
    f.txt("Hello hello HELLO hellos");
    let any = SearchFlags::default();
    assert_eq!(f.found("hello", any), [(0, 5), (6, 11), (12, 17), (18, 23)]);
    assert_eq!(
        f.found("hello", flags(true, false, false, false)),
        [(6, 11), (18, 23)]
    );
    assert_eq!(
        f.found("hello", flags(false, true, false, false)),
        [(0, 5), (6, 11), (12, 17)]
    );
    assert_eq!(f.found("", any), []);
    let (total, truncated, items) = f.search("l", any);
    assert_eq!((total, truncated, items.len()), (8, false, 8));

    f.txt("r\u{e9}sum\u{e9} resume");
    assert_eq!(f.found("resume", any), [(9, 15)]);
    assert_eq!(
        f.found("resume", flags(false, false, true, false)),
        [(0, 8), (9, 15)]
    );
    f.txt("a\u{a0}b a b");
    assert_eq!(f.found("a b", any), [(0, 4), (5, 8)]);
    assert_eq!(f.found("a b", flags(false, false, false, true)), [(5, 8)]);
}

#[test]
fn search_returns_excerpts_and_flat_positions() {
    let mut f = Find::new();
    f.txt("first line\nsecond needle line");
    let (_, _, items) = f.search("needle", SearchFlags::default());
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!((item.start, item.end), (18, 24));
    let e = &item.excerpt;
    assert_eq!(&e.text[e.start as usize..e.end as usize], "needle");
    assert!(!e.truncated_before && !e.truncated_after);
}

#[test]
fn search_counts_objects_and_breaks_as_three_bytes_and_crosses_cells() {
    // "a<object>b<line break>" then a table whose first cell holds "xb".
    let mut doc = RichDocument::new();
    let para = doc.paragraph_ids()[0];
    let section = doc.section_ids()[0];
    let (object, line) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
    let table = doc.new_table(1, 2).unwrap();
    let last = doc.new_paragraph("tail b").unwrap();
    let transaction = Transaction::new(vec![
        RichOp::InsertText {
            para,
            offset: 0,
            text: "ab".into(),
        },
        RichOp::InsertInline {
            para,
            item: Inline::Anchored(AnchoredObject {
                id: object,
                offset: 1,
                kind: "pic".into(),
                data: vec![],
            }),
        },
        RichOp::InsertInline {
            para,
            item: Inline::Break(InlineBreak {
                id: line,
                offset: 2,
                kind: BreakKind::Line,
            }),
        },
        RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: table,
        },
        RichOp::InsertBlock {
            parent: Some(section),
            position: 2,
            subtree: last,
        },
    ])
    .unwrap();
    doc.apply(&transaction).unwrap();
    let mut f = Find::with(doc);
    // Flat text: "a" FFFC "b" FFFC \n (cell) \n (cell) \n "tail b".
    let text = f.text();
    assert_eq!(text, "a\u{fffc}b\u{fffc}\n\n\ntail b");
    // A text typed into the first cell is found at its flat position.
    f.state(Command::ReplaceRange {
        start: 9,
        end: 9,
        text: "xb".into(),
    });
    let items = f.found("b", SearchFlags::default());
    check_items_read_back(&mut f, "b", &items);
    assert_eq!(items, [(4, 5), (10, 11), (18, 19)]);
    // A match never spans an object.
    assert_eq!(f.found("ab", SearchFlags::default()), []);
    // The query may be the object itself (it is a search, not an edit).
    assert_eq!(
        f.found("\u{fffc}", SearchFlags::default()),
        [(1, 4), (5, 8)]
    );
}

#[test]
fn replace_all_is_one_undo_step_and_counts() {
    let mut f = Find::new();
    f.txt("foo bar foo\nfoo");
    let (count, state) = f.replace_all("foo", "qux\nz");
    assert_eq!(count, 3);
    assert_eq!(f.text(), "qux\nz bar qux\nz\nqux\nz");
    assert_eq!(state.revision, 1);
    assert!(state.modified && state.can_undo && !state.can_redo);
    let undone = f.state(Command::Undo);
    assert_eq!(f.text(), "foo bar foo\nfoo");
    assert_eq!(undone.revision, 2);
    assert!(!undone.can_undo && undone.can_redo);
    let redone = f.state(Command::Redo);
    assert_eq!(f.text(), "qux\nz bar qux\nz\nqux\nz");
    assert_eq!(redone.revision, 3);
    // No occurrence: nothing changes, nothing is counted.
    let (count, state) = f.replace_all("absent", "x");
    assert_eq!((count, state.revision), (0, 3));
    // An occurrence that already reads like its replacement changes nothing.
    let (count, state) = f.replace_all("bar", "bar");
    assert_eq!((count, state.revision), (1, 3));
}

#[test]
fn replace_all_keeps_the_positions_of_objects_and_other_paragraphs() {
    let mut f = Find::new();
    f.txt("one two\nthree two\nfour");
    let before = f.search("two", SearchFlags::default()).2;
    assert_eq!(before.len(), 2);
    let (count, _) = f.replace_all("two", "2");
    assert_eq!(count, 2);
    assert_eq!(f.text(), "one 2\nthree 2\nfour");
    let after = f.found("2", SearchFlags::default());
    check_items_read_back(&mut f, "2", &after);
    assert_eq!(after, [(4, 5), (12, 13)]);
}

#[test]
fn replace_current_needs_the_occurrence_to_be_the_selection() {
    let mut f = Find::new();
    f.txt("cat dog cat");
    let revision = f.revision();
    let current = |revision, start, end| Command::Replace {
        revision,
        flags: SearchFlags::default(),
        start,
        end,
        query: "cat".into(),
        replacement: "bird".into(),
    };
    // Not the selection.
    assert_eq!(f.error(current(revision, 8, 11)), ErrorCode::StaleMatch);
    f.select(8, 11);
    // Stale revision.
    assert_eq!(
        f.error(current(revision + 5, 8, 11)),
        ErrorCode::StaleRevision
    );
    // A selection that is not an occurrence.
    f.select(4, 7);
    assert_eq!(f.error(current(revision, 4, 7)), ErrorCode::StaleMatch);
    f.select(8, 11);
    match f.send(current(revision, 8, 11)) {
        Response::Replaced { count, state } => {
            assert_eq!(count, 1);
            assert_eq!((state.anchor, state.focus), (12, 12));
            assert_eq!(state.revision, 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(f.text(), "cat dog bird");
    f.state(Command::Undo);
    assert_eq!(f.text(), "cat dog cat");
}

#[test]
fn replace_codec_refuses_an_empty_query_and_bad_text() {
    let request = |query: &str, replacement: &str| Request {
        doc_id: 1,
        request_id: 1,
        command: Command::ReplaceAll {
            revision: 0,
            flags: SearchFlags::default(),
            query: query.into(),
            replacement: replacement.into(),
        },
    };
    assert_eq!(
        proto::encode_request(&request("", "x")),
        Err(ErrorCode::EmptyQuery)
    );
    assert_eq!(
        proto::encode_request(&request("\u{fffc}", "x")),
        Err(ErrorCode::InvalidText)
    );
    assert_eq!(
        proto::encode_request(&request("a", "x\u{fffc}")),
        Err(ErrorCode::InvalidText)
    );
    assert_eq!(
        proto::encode_request(&request(&"a".repeat(1025), "x")),
        Err(ErrorCode::QueryTooLarge)
    );
    assert!(proto::encode_request(&request(&"a".repeat(1024), "")).is_ok());
}

#[test]
fn a_read_only_document_is_searched_and_counted_but_not_replaced() {
    let mut f = Find::new();
    let state = f.docx("revisions.docx");
    assert!(state.read_only);
    let (total, _, items) = f.search("fin", SearchFlags::default());
    assert!(total >= 1 && !items.is_empty());
    let revision = f.revision();
    assert_eq!(
        f.error(Command::ReplaceAll {
            revision,
            flags: SearchFlags::default(),
            query: "fin".into(),
            replacement: "x".into(),
        }),
        ErrorCode::ReadOnly
    );
    f.select(items[0].start, items[0].end);
    assert_eq!(
        f.error(Command::Replace {
            revision,
            flags: SearchFlags::default(),
            start: items[0].start,
            end: items[0].end,
            query: "fin".into(),
            replacement: "x".into(),
        }),
        ErrorCode::ReadOnly
    );
    let mut changes: [Option<String>; proto::PROPERTY_COUNT] = Default::default();
    changes[0] = Some("t".into());
    assert_eq!(
        f.error(Command::PropertiesApply { changes }),
        ErrorCode::ReadOnly
    );
    // Reading the properties and the statistics is allowed.
    assert!(!f.properties().limits.contains(&0));
    assert!(matches!(
        f.send(Command::Statistics),
        Response::Statistics { .. }
    ));
}

#[test]
fn statistics_count_the_document_and_the_selection() {
    let mut f = Find::new();
    f.txt("one two three\nfour");
    f.select(0, 7);
    let Response::Statistics {
        revision,
        anchor,
        focus,
        layout_complete,
        document,
        selection,
    } = f.send(Command::Statistics)
    else {
        panic!("expected statistics");
    };
    assert_eq!((revision, anchor, focus), (0, 0, 7));
    assert!(layout_complete);
    assert_eq!(document.words, 4);
    assert_eq!(document.paragraphs, 2);
    assert_eq!(document.characters, 15);
    assert_eq!(document.characters_with_spaces, 17);
    assert_eq!((document.lines, document.pages), (2, 1));
    assert_eq!(selection.words, 2);
    assert_eq!(selection.characters, 6);
    assert_eq!(selection.characters_with_spaces, 7);
    assert_eq!(selection.paragraphs, 1);
    assert_eq!((selection.lines, selection.pages), (1, 1));
    // A selection over two paragraphs touches two lines.
    f.select(4, 16);
    let Response::Statistics { selection, .. } = f.send(Command::Statistics) else {
        panic!("expected statistics");
    };
    assert_eq!((selection.paragraphs, selection.lines), (2, 2));
    // A caret selects nothing.
    f.select(3, 3);
    let Response::Statistics { selection, .. } = f.send(Command::Statistics) else {
        panic!("expected statistics");
    };
    assert_eq!(selection, proto::StatCounts::default());
}

#[test]
fn statistics_ignore_objects_in_the_selection() {
    let mut f = Find::with({
        let mut doc = RichDocument::new();
        let para = doc.paragraph_ids()[0];
        let object = doc.alloc_id().unwrap();
        let transaction = Transaction::new(vec![
            RichOp::InsertText {
                para,
                offset: 0,
                text: "ab cd".into(),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Anchored(AnchoredObject {
                    id: object,
                    offset: 2,
                    kind: "pic".into(),
                    data: vec![],
                }),
            },
        ])
        .unwrap();
        doc.apply(&transaction).unwrap();
        doc
    });
    f.select(0, 8);
    let Response::Statistics {
        document,
        selection,
        ..
    } = f.send(Command::Statistics)
    else {
        panic!("expected statistics");
    };
    assert_eq!((document.words, document.characters), (2, 4));
    assert_eq!((selection.words, selection.characters), (2, 4));
}

#[test]
fn search_rects_highlight_the_occurrences_of_a_page() {
    let mut f = Find::new();
    f.txt("alpha beta\ngamma beta\nbeta");
    let (_, _, items) = f.search("beta", SearchFlags::default());
    assert_eq!(items.len(), 3);
    let ranges: Vec<(u64, u64)> = items.iter().map(|i| (i.start, i.end)).collect();
    let revision = f.revision();
    let rects = |f: &mut Find, revision, first_page, ranges: &[(u64, u64)]| {
        f.send(Command::SearchRects {
            revision,
            first_page,
            page_count: 1,
            ranges: ranges.to_vec(),
        })
    };
    let Response::SearchRects { rects: found, .. } = rects(&mut f, revision, 0, &ranges) else {
        panic!("expected rectangles");
    };
    assert!(found.len() >= 3, "{found:?}");
    for rect in &found {
        assert!(rect.item < 3 && rect.page == 0 && rect.width > 0.0 && rect.height > 0.0);
    }
    let by = |rects: &[HighlightRect], item| rects.iter().filter(|r| r.item == item).count();
    assert!((0..3).all(|i| by(&found, i) >= 1));
    // The second line is lower than the first.
    let y = |item| found.iter().find(|r| r.item == item).unwrap().y;
    assert!(y(0) < y(1) && y(1) < y(2));
    assert_eq!(
        f.error(Command::SearchRects {
            revision: revision + 1,
            first_page: 0,
            page_count: 1,
            ranges: ranges.clone(),
        }),
        ErrorCode::StaleRevision
    );
    assert_eq!(
        f.error(Command::SearchRects {
            revision,
            first_page: 40,
            page_count: 1,
            ranges: ranges.clone(),
        }),
        ErrorCode::PageRange
    );
    assert_eq!(
        f.error(Command::SearchRects {
            revision,
            first_page: 0,
            page_count: 1,
            ranges: vec![(0, 9999)],
        }),
        ErrorCode::PositionOutOfRange
    );
}

#[test]
fn properties_are_read_applied_saved_and_make_the_document_modified() {
    let mut f = Find::new();
    f.open(OpenKind::Blank, Profile::Document, &[]);
    let before = f.properties();
    assert_eq!(before.editable[0], "");
    assert_eq!(before.limits[0], 255);
    let outcome = f.apply(0, "My title");
    assert_eq!(outcome.status, PropertiesStatus::Applied);
    assert_eq!(outcome.changed, 1);
    assert!(outcome.state.modified, "a property change is a change");
    assert_eq!(outcome.state.revision, 0);
    assert_eq!(f.properties().editable[0], "My title");
    // The same value again changes nothing.
    assert_eq!(f.apply(0, "My title").changed, 0);
    // Too long: nothing applied, the limit and the length come back.
    let long = "x".repeat(300);
    let refused = f.apply(1, &long);
    assert_eq!(refused.status, PropertiesStatus::TooLong);
    assert_eq!((refused.field, refused.max, refused.len), (1, 255, 300));
    assert_eq!(f.properties().editable[1], "");
    // The saved DOCX holds the property, and saving clears the modified flag.
    let bytes = f.save(SaveFormat::Docx);
    let title = Package::read(&bytes)
        .unwrap()
        .properties_snapshot()
        .editable
        .title;
    assert_eq!(title, "My title");
    let state = f.state(Command::MarkSaved { revision: 0 });
    assert!(!state.modified);
    // A change made after the save keeps the document modified even if the
    // client marks the older save.
    let saved_before = f.save(SaveFormat::Docx);
    f.apply(2, "Someone");
    let state = f.state(Command::MarkSaved { revision: 0 });
    assert!(state.modified);
    drop(saved_before);
    let bytes = f.save(SaveFormat::Docx);
    assert_eq!(
        Package::read(&bytes)
            .unwrap()
            .properties_snapshot()
            .editable
            .author,
        "Someone"
    );
    assert!(!f.state(Command::MarkSaved { revision: 0 }).modified);
}

#[test]
fn properties_of_an_opened_docx_are_edited_in_its_package() {
    let mut f = Find::new();
    f.docx("basic.docx");
    let snapshot = f.properties();
    // Live counts replace the stored ones.
    assert!(snapshot.statistics[1].is_some());
    let outcome = f.apply(6, "alpha, beta");
    assert_eq!(outcome.changed, 1);
    let bytes = f.save(SaveFormat::Docx);
    let reopened = Package::read(&bytes).unwrap().properties_snapshot();
    assert_eq!(reopened.editable.keywords, "alpha, beta");
    // A text edit and a property edit are independent.
    f.state(Command::InsertText { text: "x".into() });
    assert_eq!(f.properties().editable[6], "alpha, beta");
}

#[test]
fn undo_back_to_the_saved_point_makes_the_document_clean_again() {
    let mut f = Find::new();
    f.txt("");
    let typed = f.state(Command::InsertText { text: "ab".into() });
    assert!(typed.modified);
    let saved = f.save(SaveFormat::Txt);
    assert!(!saved.is_empty());
    assert!(!f.state(Command::MarkSaved { revision: 1 }).modified);
    let more = f.state(Command::InsertText { text: "cd".into() });
    assert!(more.modified);
    let undone = f.state(Command::Undo);
    assert_eq!(f.text(), "ab");
    assert!(
        undone.revision > 1 && !undone.modified,
        "the history is back at the saved point"
    );
    assert!(f.state(Command::Redo).modified);
    assert!(!f.state(Command::Undo).modified);
    // Undoing past the saved point is a change; redoing returns to it.
    assert!(f.state(Command::Undo).modified);
    assert!(!f.state(Command::Redo).modified);
    // A replacement undone is clean again too.
    f.state(Command::InsertText { text: " ab".into() });
    let revision = f.revision();
    f.state(Command::MarkSaved { revision });
    let done = f.replace_all("ab", "x");
    assert!(done.1.modified);
    assert!(!f.state(Command::Undo).modified);
}

#[test]
fn a_text_save_does_not_take_the_properties() {
    let mut f = Find::new();
    f.open(OpenKind::Blank, Profile::Document, &[]);
    assert!(f.apply(0, "Titre").state.modified);
    f.save(SaveFormat::Txt);
    assert!(
        f.state(Command::MarkSaved { revision: 0 }).modified,
        "the title is in no file yet"
    );
    f.save(SaveFormat::Docx);
    assert!(!f.state(Command::MarkSaved { revision: 0 }).modified);
}
