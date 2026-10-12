// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hand-computed tests of the text search (US-340). Offsets are written out by
//! hand from the UTF-8 encoding of each text.

mod common;

use atelier_text_model::rich::{
    Field, HfKind, HfVariant, Inline, Node, NodeId, RichDocument, RichOp,
};
use atelier_text_model::search::{LineId, Position, SearchOptions, Searcher, TextLines, Zone};
use atelier_text_model::{PlainTextDocument, PlainTextLimits};
use common::*;

fn opts() -> SearchOptions {
    SearchOptions {
        context_graphemes: 0,
        ..SearchOptions::default()
    }
}

/// Global byte ranges of the occurrences of `query` in plain `text`.
fn find(text: &str, query: &str, options: SearchOptions) -> Vec<(usize, usize)> {
    let lines = TextLines(text);
    Searcher::new(&lines, query, options)
        .iter()
        .map(|o| {
            let r = o.global_range();
            (r.start, r.end)
        })
        .collect()
}

#[test]
fn case_option() {
    let t = "Hello hello HELLO";
    assert_eq!(find(t, "hello", opts()), [(0, 5), (6, 11), (12, 17)]);
    let cs = SearchOptions {
        match_case: true,
        ..opts()
    };
    assert_eq!(find(t, "hello", cs), [(6, 11)]);
    assert_eq!(find(t, "HELLO", cs), [(12, 17)]);
}

#[test]
fn sharp_s_both_ways() {
    // s0 t1 r2 a3 ß4..6 e6, length 7.
    assert_eq!(find("straße", "STRASSE", opts()), [(0, 7)]);
    assert_eq!(find("STRASSE", "straße", opts()), [(0, 7)]);
    assert_eq!(find("straße", "ss", opts()), [(4, 6)]);
    assert_eq!(find("straße", "asse", opts()), [(3, 7)]);
    assert_eq!(find("straße", "sse", opts()), [(4, 7)]);
    // The second half of the sharp s is inside a grapheme.
    assert_eq!(find("straße", "se", opts()), []);
    assert_eq!(find("straße", "s", opts()), [(0, 1)]);
    // Case-sensitive: no folding, ß only matches ß.
    let cs = SearchOptions {
        match_case: true,
        ..opts()
    };
    assert_eq!(find("straße", "strasse", cs), []);
    assert_eq!(find("straße", "ß", cs), [(4, 6)]);
}

#[test]
fn ligature() {
    // fi ligature U+FB01 is 3 bytes, then n.
    assert_eq!(find("\u{fb01}n", "fin", opts()), [(0, 4)]);
    assert_eq!(find("\u{fb01}n", "fi", opts()), [(0, 3)]);
    assert_eq!(find("\u{fb01}n", "f", opts()), []);
    assert_eq!(find("\u{fb01}n", "in", opts()), []);
    assert_eq!(find("fin", "\u{fb01}", opts()), [(0, 2)]);
    let cs = SearchOptions {
        match_case: true,
        ..opts()
    };
    assert_eq!(find("\u{fb01}n", "fi", cs), []);
    assert_eq!(find("\u{fb01}n", "\u{fb01}", cs), [(0, 3)]);
}

#[test]
fn composed_and_decomposed_forms_find_each_other() {
    let nfc = "\u{e9}l\u{e8}ve"; // é0..2 l2 è3..5 v5 e6
    let nfd = "e\u{301}le\u{300}ve"; // e0 ◌́1..3 l3 e4 ◌̀5..7 v7 e8
    assert_eq!(find(nfc, nfc, opts()), [(0, 7)]);
    assert_eq!(find(nfc, nfd, opts()), [(0, 7)]);
    assert_eq!(find(nfd, nfc, opts()), [(0, 9)]);
    assert_eq!(find(nfd, nfd, opts()), [(0, 9)]);
    assert_eq!(find(nfc, "eleve", opts()), []);
    assert_eq!(find(nfd, "eleve", opts()), []);
}

#[test]
fn ignore_accents() {
    let ia = SearchOptions {
        ignore_accents: true,
        ..opts()
    };
    let nfc = "\u{e9}l\u{e8}ve";
    let nfd = "e\u{301}le\u{300}ve";
    assert_eq!(find(nfc, "eleve", ia), [(0, 7)]);
    assert_eq!(find(nfd, "eleve", ia), [(0, 9)]);
    assert_eq!(find("eleve", nfc, ia), [(0, 5)]);
    assert_eq!(find("eleve", "\u{e9}", ia), [(0, 1), (2, 3), (4, 5)]);
    // Combined with case folding.
    assert_eq!(find("\u{c9}L\u{c8}VE", "eleve", ia), [(0, 7)]);
    let both = SearchOptions {
        match_case: true,
        ..ia
    };
    assert_eq!(find("\u{c9}L\u{c8}VE", "eleve", both), []);
    assert_eq!(find("\u{c9}L\u{c8}VE", "ELEVE", both), [(0, 7)]);
}

#[test]
fn combining_accent_is_part_of_its_grapheme() {
    let t = "e\u{301}"; // 3 bytes
    assert_eq!(find(t, "e", opts()), []);
    let ia = SearchOptions {
        ignore_accents: true,
        ..opts()
    };
    assert_eq!(find(t, "e", ia), [(0, 3)]);
    // "e" + accent, then "e": only the second one is a plain e.
    let t = "e\u{301}e";
    assert_eq!(find(t, "e", opts()), [(3, 4)]);
    assert_eq!(find(t, "e", ia), [(0, 3), (3, 4)]);
    // Query with an accent against text without.
    assert_eq!(find("e", "\u{e9}", opts()), []);
    assert_eq!(find("e", "\u{e9}", ia), [(0, 1)]);
    // A standalone mark at the start of the paragraph is ignored as a start.
    assert_eq!(find("\u{301}a", "a", ia), [(2, 3)]);
    // A query made only of ignored marks matches nothing.
    assert_eq!(find("e\u{301}", "\u{301}", ia), []);
}

#[test]
fn turkish_dotted_capital_i() {
    // İ is 2 bytes and folds to i + U+0307.
    let t = "\u{130}stanbul";
    assert_eq!(find(t, "i\u{307}stanbul", opts()), [(0, 9)]);
    assert_eq!(find(t, "istanbul", opts()), []);
    assert_eq!(find(t, "i", opts()), []);
    let ia = SearchOptions {
        ignore_accents: true,
        ..opts()
    };
    assert_eq!(find(t, "istanbul", ia), [(0, 9)]);
    assert_eq!(find(t, "i", ia), [(0, 2)]);
}

#[test]
fn no_break_spaces() {
    let nb = "a\u{a0}b"; // 4 bytes
    let nnb = "a\u{202f}b"; // 5 bytes
    assert_eq!(find(nb, "a b", opts()), [(0, 4)]);
    assert_eq!(find(nnb, "a b", opts()), [(0, 5)]);
    assert_eq!(find("a b", "a\u{a0}b", opts()), [(0, 3)]);
    let strict = SearchOptions {
        match_nbsp: true,
        ..opts()
    };
    assert_eq!(find(nb, "a b", strict), []);
    assert_eq!(find(nnb, "a b", strict), []);
    assert_eq!(find("a b", "a\u{a0}b", strict), []);
    assert_eq!(find(nb, "a\u{a0}b", strict), [(0, 4)]);
    assert_eq!(find(nb, "a\u{202f}b", strict), []);
    assert_eq!(find("a b", "a b", strict), [(0, 3)]);
}

#[test]
fn whole_word_uax29() {
    let ww = SearchOptions {
        whole_word: true,
        ..opts()
    };
    let t = "le chat, les chats";
    assert_eq!(find(t, "chat", opts()), [(3, 7), (13, 17)]);
    assert_eq!(find(t, "chat", ww), [(3, 7)]);
    assert_eq!(find(t, "chats", ww), [(13, 18)]);
    // An apostrophe between letters does not break a word.
    assert_eq!(find("l'eau", "eau", ww), []);
    assert_eq!(find("l'eau", "l'eau", ww), [(0, 5)]);
    assert_eq!(find("don't", "don", ww), []);
    // A hyphen does.
    assert_eq!(find("trait-d'union", "trait", ww), [(0, 5)]);
    assert_eq!(find("trait-d'union", "d", ww), []);
    assert_eq!(find("trait-d'union", "d'union", ww), [(6, 13)]);
    assert_eq!(find("trait-d'union", "union", ww), []);
    assert_eq!(find("a-b", "b", ww), [(2, 3)]);
    // The ends of the paragraph are boundaries.
    assert_eq!(find("chat", "chat", ww), [(0, 4)]);
    // A query that is itself a separator.
    assert_eq!(find("a b", " ", ww), [(1, 2)]);
    // Digits and letters stay together.
    assert_eq!(find("abc123 abc", "abc", ww), [(7, 10)]);
}

#[test]
fn emoji_and_surrogate_pairs() {
    // x0 😀1..5 y5 😀6..10
    assert_eq!(find("x😀y😀", "😀", opts()), [(1, 5), (6, 10)]);
    let family = "👨\u{200d}👩\u{200d}👧"; // 4+3+4+3+4 = 18 bytes
    assert_eq!(find(family, "👨", opts()), []);
    assert_eq!(find(family, "👧", opts()), []);
    assert_eq!(find(family, family, opts()), [(0, 18)]);
    let t = format!("a{family}b");
    assert_eq!(find(&t, family, opts()), [(1, 19)]);
    // Regional indicators pair up from the start of the run.
    let flags = "🇫🇷🇫🇷";
    assert_eq!(find(flags, "🇫🇷", opts()), [(0, 8), (8, 16)]);
    assert_eq!(find(flags, "🇷🇫", opts()), []);
    // Skin tone modifier stays with its base.
    assert_eq!(find("👍🏽", "👍", opts()), []);
}

#[test]
fn empty_text_and_query() {
    assert_eq!(find("", "a", opts()), []);
    assert_eq!(find("", "", opts()), []);
    assert_eq!(find("abc", "", opts()), []);
    let lines = TextLines("abc");
    let s = Searcher::new(&lines, "", opts());
    assert!(s.is_empty_query());
    assert_eq!(s.iter().count(), 0);
    assert!(
        s.next_from(&Position {
            paragraph: LineId(0),
            offset: 0
        })
        .is_none()
    );
    assert!(
        s.previous_before(&Position {
            paragraph: LineId(0),
            offset: 3
        })
        .is_none()
    );
    assert_eq!(s.find_all(10).len(), 0);
    let empty = TextLines("");
    let s = Searcher::new(&empty, "a", opts());
    assert!(
        s.next_from(&Position {
            paragraph: LineId(0),
            offset: 0
        })
        .is_none()
    );
    assert!(
        s.previous_before(&Position {
            paragraph: LineId(0),
            offset: 0
        })
        .is_none()
    );
}

#[test]
fn paragraph_starts_ends_and_limits() {
    // "ab\ncd\nab\r\nab\rab"
    let t = "ab\ncd\nab\r\nab\rab";
    assert_eq!(find(t, "ab", opts()), [(0, 2), (6, 8), (10, 12), (13, 15)]);
    // Occurrences never cross a paragraph boundary.
    assert_eq!(find("a\nb", "a\nb", opts()), []);
    assert_eq!(find("a\nb", "ab", opts()), []);
    assert_eq!(find("a\r\nb", "a b", opts()), []);
    let doc = PlainTextDocument::from_utf8(b"x\nab", PlainTextLimits::default()).unwrap();
    let s = Searcher::new(&doc, "ab", opts());
    let all = s.find_all(5);
    assert_eq!(all.items[0].paragraph, LineId(2));
    assert_eq!(all.items[0].range, 0..2);
    assert_eq!(all.items[0].global_range(), 2..4);
}

#[test]
fn matches_do_not_overlap() {
    assert_eq!(find("aaaa", "aa", opts()), [(0, 2), (2, 4)]);
    assert_eq!(find("aaa", "aa", opts()), [(0, 2)]);
    assert_eq!(find("ababa", "aba", opts()), [(0, 3)]);
    assert_eq!(find("abababab", "abab", opts()), [(0, 4), (4, 8)]);
    // A rejected candidate does not consume text: the first "aa" ends inside
    // the grapheme "a" + accent, so it is skipped and the later one is taken.
    assert_eq!(find("aa\u{301}aa", "aa", opts()), [(4, 6)]);
}

#[test]
fn excerpts() {
    let o = SearchOptions {
        context_graphemes: 3,
        ..SearchOptions::default()
    };
    let lines = TextLines("0123456789 needle abcdefghij");
    let s = Searcher::new(&lines, "needle", o);
    let e = s.iter().next().unwrap().excerpt.unwrap();
    assert_eq!(e.text, "89 needle ab");
    assert_eq!(&e.text[e.start..e.end], "needle");
    assert!(e.truncated_before && e.truncated_after);

    let lines = TextLines("needle x\nneedle");
    let s = Searcher::new(&lines, "needle", o);
    let all = s.find_all(9);
    let e = all.items[0].excerpt.as_ref().unwrap();
    assert_eq!((e.text.as_str(), e.start, e.end), ("needle x", 0, 6));
    assert!(!e.truncated_before && !e.truncated_after);
    let e = all.items[1].excerpt.as_ref().unwrap();
    assert_eq!((e.text.as_str(), e.start, e.end), ("needle", 0, 6));

    // Graphemes, not bytes or scalars.
    let lines = TextLines("e\u{301}e\u{301}needle");
    let o1 = SearchOptions {
        context_graphemes: 1,
        ..SearchOptions::default()
    };
    let e = Searcher::new(&lines, "needle", o1)
        .iter()
        .next()
        .unwrap()
        .excerpt
        .unwrap();
    assert_eq!(e.text, "e\u{301}needle");
    assert_eq!((e.start, e.end), (3, 9));
    assert!(e.truncated_before && !e.truncated_after);

    // Disabled.
    let s = Searcher::new(&lines, "needle", opts());
    assert!(s.iter().next().unwrap().excerpt.is_none());
}

#[test]
fn cap_and_count() {
    let lines = TextLines("a a a");
    let s = Searcher::new(&lines, "a", opts());
    let r = s.find_all(2);
    assert_eq!((r.len(), r.truncated), (2, true));
    let r = s.find_all(3);
    assert_eq!((r.len(), r.truncated), (3, false));
    let r = s.find_all(0);
    assert_eq!((r.len(), r.truncated), (0, true));
    assert_eq!(s.count(10), (3, false));
    assert_eq!(s.count(3), (3, false));
    assert_eq!(s.count(2), (2, true));
}

#[test]
fn navigation_and_wrapping() {
    // Lines: "x"@0, "ab"@2, "y"@5, "ab"@7.
    let text = "x\nab\ny\nab";
    let lines = TextLines(text);
    let s = Searcher::new(&lines, "ab", opts());
    let all = s.find_all(10);
    let at = |g: usize| lines.position(g);
    let next = |g: usize| {
        let st = s.next_from(&at(g)).unwrap();
        (st.occurrence.global_range(), st.wrapped)
    };
    let prev = |g: usize| {
        let st = s.previous_before(&at(g)).unwrap();
        (st.occurrence.global_range(), st.wrapped)
    };
    assert_eq!(next(0), (2..4, false));
    assert_eq!(next(2), (2..4, false));
    assert_eq!(next(3), (7..9, false));
    assert_eq!(next(8), (2..4, true));
    assert_eq!(next(9), (2..4, true));
    assert_eq!(prev(9), (7..9, false));
    assert_eq!(prev(7), (2..4, false));
    assert_eq!(prev(2), (7..9, true));
    assert_eq!(prev(0), (7..9, true));
    // "3 of 17": the index in the list.
    let st = s.next_from(&at(3)).unwrap();
    assert_eq!(all.index_of(&st.occurrence), Some(1));
    let st = s.next_from(&at(0)).unwrap();
    assert_eq!(all.index_of(&st.occurrence), Some(0));
    // An unknown paragraph means the start (next) or the end (previous).
    let ghost = Position {
        paragraph: LineId(1234),
        offset: 0,
    };
    assert_eq!(s.next_from(&ghost).unwrap().occurrence.global_range(), 2..4);
    assert_eq!(
        s.previous_before(&ghost).unwrap().occurrence.global_range(),
        7..9
    );
    // One single occurrence: it wraps onto itself.
    let one = TextLines("ab");
    let s = Searcher::new(&one, "ab", opts());
    let st = s.next_from(&one.position(0)).unwrap();
    assert!(!st.wrapped);
    let st = s.next_from(&one.position(1)).unwrap();
    assert!(st.wrapped);
    let st = s.previous_before(&one.position(0)).unwrap();
    assert!(st.wrapped);
}

#[test]
fn text_lines_positions() {
    let l = TextLines("ab\r\ncd\nef\rgh");
    assert_eq!(
        l.position(0),
        Position {
            paragraph: LineId(0),
            offset: 0
        }
    );
    assert_eq!(
        l.position(2),
        Position {
            paragraph: LineId(0),
            offset: 2
        }
    );
    // Inside the CRLF pair: moved before it.
    assert_eq!(
        l.position(3),
        Position {
            paragraph: LineId(0),
            offset: 2
        }
    );
    assert_eq!(
        l.position(4),
        Position {
            paragraph: LineId(4),
            offset: 0
        }
    );
    assert_eq!(
        l.position(10),
        Position {
            paragraph: LineId(10),
            offset: 0
        }
    );
    assert_eq!(
        l.position(99),
        Position {
            paragraph: LineId(10),
            offset: 2
        }
    );
}

fn set_text(doc: &mut RichDocument, para: NodeId, text: &str) {
    run_checked(
        doc,
        RichOp::InsertText {
            para,
            offset: 0,
            text: text.to_owned(),
        },
    );
}

#[test]
fn rich_document_order_zones_and_fields() {
    let mut doc = RichDocument::new();
    let s1 = first_section(&doc);
    let body = first_para(&doc);
    set_text(&mut doc, body, "needle body");

    let header = doc.new_header_footer("needle header").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section: s1,
            kind: HfKind::Header,
            variant: HfVariant::Default,
            subtree: header,
        },
    );
    let footer = doc.new_header_footer("footer needle").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section: s1,
            kind: HfKind::Footer,
            variant: HfVariant::Default,
            subtree: footer,
        },
    );

    let table = add_table(&mut doc, 2, 2);
    let grid = cells(&doc, table);
    let cell_para = |doc: &RichDocument, cell: NodeId| doc.block_ids(Some(cell)).unwrap()[0];
    let p00 = cell_para(&doc, grid[0][0]);
    set_text(&mut doc, p00, "needle c00");
    let p01 = cell_para(&doc, grid[0][1]);
    set_text(&mut doc, p01, "nothing");
    // A nested table in the cell (1,1), after its own paragraph.
    let nested = doc.new_table(1, 1).unwrap();
    let nested_cell = cells_of(&nested);
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(grid[1][1]),
            position: 1,
            subtree: nested,
        },
    );
    let pn = cell_para(&doc, nested_cell);
    set_text(&mut doc, pn, "needle nested");

    let after = doc.new_paragraph("needle after").unwrap();
    let after_id = after.root;
    let n = doc.block_ids(Some(s1)).unwrap().len();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(s1),
            position: n,
            subtree: after,
        },
    );

    // A second section with a header of its own.
    let s2 = doc.new_section().unwrap();
    let s2_id = s2.root;
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: None,
            position: 1,
            subtree: s2,
        },
    );
    let p2 = doc.block_ids(Some(s2_id)).unwrap()[0];
    set_text(&mut doc, p2, "needle second");
    let h2 = doc.new_header_footer("needle header 2").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section: s2_id,
            kind: HfKind::Header,
            variant: HfVariant::Default,
            subtree: h2,
        },
    );
    doc.validate().unwrap();

    let s = Searcher::new(&doc, "NEEDLE", opts());
    let got: Vec<(usize, Zone, String)> = s
        .iter()
        .map(|o| {
            let p = doc.paragraph(o.paragraph).unwrap();
            assert_eq!(p.text[o.range.clone()].to_lowercase(), "needle");
            (o.section, o.zone, p.text.clone())
        })
        .collect();
    let expect = [
        (0, Zone::Header, "needle header"),
        (0, Zone::Body, "needle body"),
        (0, Zone::Table, "needle c00"),
        (0, Zone::Table, "needle nested"),
        (0, Zone::Body, "needle after"),
        (0, Zone::Footer, "footer needle"),
        (1, Zone::Header, "needle header 2"),
        (1, Zone::Body, "needle second"),
    ];
    let expect: Vec<_> = expect
        .iter()
        .map(|(a, b, c)| (*a, *b, c.to_string()))
        .collect();
    assert_eq!(got, expect);
    // Paragraph ids in the occurrences are those of the model.
    let ids: Vec<NodeId> = s.iter().map(|o| o.paragraph).collect();
    assert_eq!(ids[1], body);
    assert_eq!(ids[4], after_id);
    assert_eq!(ids[7], p2);

    // Navigation walks the same list.
    let all = s.find_all(100);
    let mut pos = Position {
        paragraph: body,
        offset: 0,
    };
    // Start from the very first paragraph of the document: the header.
    let first = s.iter().next().unwrap();
    pos.paragraph = first.paragraph;
    let mut walked = Vec::new();
    for _ in 0..all.len() {
        let st = s.next_from(&pos).unwrap();
        walked.push(st.occurrence.clone());
        pos = Position {
            paragraph: st.occurrence.paragraph,
            offset: st.occurrence.range.end,
        };
    }
    assert_eq!(walked, all.items);
    assert!(s.next_from(&pos).unwrap().wrapped);

    // Field: the result text is searchable, its code is not.
    let fp = first_para(&doc);
    let id = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: fp,
            item: Inline::Field(Field {
                id,
                start: 0,
                end: 6,
                instruction: "PAGEREF zebra".to_owned(),
            }),
        },
    );
    assert_eq!(Searcher::new(&doc, "zebra", opts()).iter().count(), 0);
    let r = Searcher::new(&doc, "needle", opts()).find_all(100);
    assert_eq!(r.items[1].range, 0..6); // the field result "needle"
    assert_eq!(r.len(), 8);
    // Text and ranges give what US-341 needs: the attributes at the start.
    let attrs = doc.paragraph(fp).unwrap().attrs_at(r.items[1].range.start);
    assert!(attrs.is_empty());
}

fn cells_of(sub: &atelier_text_model::rich::Subtree) -> NodeId {
    sub.nodes
        .iter()
        .find_map(|(id, n)| matches!(n, Node::Cell(_)).then_some(*id))
        .unwrap()
}
