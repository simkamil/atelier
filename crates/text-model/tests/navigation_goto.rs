// SPDX-License-Identifier: AGPL-3.0-or-later
//! Go To (US-291). Expected values computed by hand from the fixture below.
//!
//! Section 0 body, reading ranks: p0 "Chapter one" (heading 1) 0, p1 "text" 1,
//! table T1 2x2 (c00 2, c01 3, c10 4, c11 5), p2 "after table" 6 (object at
//! 2), p3 "Heading two" 7 (heading 2, object at 0). Section 1: p4 8 (heading
//! 1), table T2 1x1 (t2c 9), p5 10. Pages start at p0, c10, p3, p4, p5;
//! lines at p0, p1, c00, p2, p3, p4.

mod common;
mod nav_common;

use atelier_model::selection::TextPosition;
use atelier_text_model::navigation::*;
use atelier_text_model::rich::*;
use atelier_text_model::search::Zone;
use common::*;
use nav_common::*;

struct Fx {
    doc: RichDocument,
    map: FixedPageMap,
    p: [NodeId; 6],
    c: [NodeId; 4],
    t2c: NodeId,
}

fn fixture() -> Fx {
    let (mut doc, p0) = with_text("Chapter one");
    set_style(&mut doc, p0, "atelier.heading.1");
    let s0 = first_section(&doc);
    let p1 = add_para(&mut doc, s0, "text");
    let t1 = add_table_to(&mut doc, s0, 2, 2);
    let grid = cells(&doc, t1);
    let c = [
        cell_para(&doc, grid[0][0]),
        cell_para(&doc, grid[0][1]),
        cell_para(&doc, grid[1][0]),
        cell_para(&doc, grid[1][1]),
    ];
    let p2 = add_para(&mut doc, s0, "after table");
    add_object(&mut doc, p2, 2);
    let p3 = add_para(&mut doc, s0, "Heading two");
    set_style(&mut doc, p3, "atelier.heading.2");
    add_object(&mut doc, p3, 0);
    let (s1, p4) = add_section(&mut doc);
    set_text(&mut doc, p4, "Second section");
    set_style(&mut doc, p4, "Heading1");
    let t2 = add_table_to(&mut doc, s1, 1, 1);
    let t2c = cell_para(&doc, cells(&doc, t2)[0][0]);
    let p5 = add_para(&mut doc, s1, "end");
    let map = FixedPageMap::new(
        &doc,
        vec![pos(p0, 0), pos(c[2], 0), pos(p3, 0), pos(p4, 0), pos(p5, 0)],
        vec![
            pos(p0, 0),
            pos(p1, 0),
            pos(c[0], 0),
            pos(p2, 0),
            pos(p3, 0),
            pos(p4, 0),
        ],
    );
    Fx {
        doc,
        map,
        p: [p0, p1, p2, p3, p4, p5],
        c,
        t2c,
    }
}

fn reach(f: &Fx, from: (NodeId, usize), kind: GotoKind, input: &str) -> Landing {
    match goto(&f.doc, &f.map, pos(from.0, from.1), kind, input).unwrap() {
        GotoOutcome::Reached(l) => l,
        GotoOutcome::NoElements => panic!("{kind:?} {input:?}: no element"),
    }
}

/// (node, offset, index, clamp)
fn at(l: &Landing) -> (NodeId, usize, usize, Option<Clamp>) {
    (l.position.node, l.position.offset, l.index, l.clamp)
}

#[test]
fn parse_amounts() {
    use GotoAmount::*;
    let p = |k, s| parse_amount(k, s);
    assert_eq!(p(GotoKind::Page, ""), Ok(Relative(1)));
    assert_eq!(p(GotoKind::Page, "   "), Ok(Relative(1)));
    assert_eq!(p(GotoKind::Table, "12"), Ok(Absolute(12)));
    assert_eq!(p(GotoKind::Table, " 007 "), Ok(Absolute(7)));
    assert_eq!(p(GotoKind::Table, "+3"), Ok(Relative(3)));
    assert_eq!(p(GotoKind::Table, "-2"), Ok(Relative(-2)));
    assert_eq!(p(GotoKind::Table, " + 3 "), Ok(Relative(3)));
    assert_eq!(p(GotoKind::Table, "+0"), Ok(Relative(0)));
    assert_eq!(p(GotoKind::Table, "0"), Ok(Absolute(0)));
    assert_eq!(
        p(GotoKind::Page, "p3s2"),
        Ok(PageInSection {
            page: 3,
            section: 2
        })
    );
    assert_eq!(
        p(GotoKind::Page, " P 3 S 2 "),
        Ok(PageInSection {
            page: 3,
            section: 2
        })
    );
    for bad in [
        "abc",
        "3.5",
        "++3",
        "+",
        "-",
        "+-3",
        "1 2",
        "٣",
        "1e3",
        "0x10",
        "+ +3",
        "\u{a0}+3x",
    ] {
        assert_eq!(
            p(GotoKind::Line, bad),
            Err(ParseError::InvalidNumber),
            "{bad:?}"
        );
    }
    for bad in [
        "p", "p3", "p3s", "ps2", "p3s2x", "s2", "p-1s1", "p3s2s1", "p+3s1",
    ] {
        assert_eq!(
            p(GotoKind::Page, bad),
            Err(ParseError::InvalidNumber),
            "{bad:?}"
        );
    }
    // Compound forms exist for pages only.
    assert_eq!(p(GotoKind::Section, "p3s2"), Err(ParseError::InvalidNumber));
    assert_eq!(p(GotoKind::Line, "p3s2"), Err(ParseError::InvalidNumber));
    for big in [
        "1000000001",
        "99999999999999999999999",
        "-1000000001",
        "+99999999999999999999999",
    ] {
        assert_eq!(p(GotoKind::Line, big), Err(ParseError::TooLarge), "{big}");
    }
    assert_eq!(p(GotoKind::Line, "1000000000"), Ok(Absolute(1_000_000_000)));
}

#[test]
fn heading_styles() {
    for yes in [
        "atelier.heading.1",
        "atelier.heading.9",
        "Heading1",
        "heading 2",
        "Titre3",
        "TITRE 9",
    ] {
        assert!(is_heading_style(yes), "{yes}");
    }
    for no in [
        "atelier.normal",
        "atelier.title",
        "Heading",
        "Heading0",
        "Heading10",
        "atelier.heading.",
        "Normal",
    ] {
        assert!(!is_heading_style(no), "{no}");
    }
}

#[test]
fn sections() {
    let f = fixture();
    let [p0, p1, _, p3, p4, _] = f.p;
    // Empty = next; from section 0 it is section 2.
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Section, "")),
        (p4, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Section, "2")),
        (p4, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Section, "1")),
        (p0, 0, 1, None)
    );
    // +1 from the last section: stays there, clamped.
    assert_eq!(
        at(&reach(&f, (p4, 3), GotoKind::Section, "+1")),
        (p4, 0, 2, Some(Clamp::ToLast))
    );
    // -1 from inside section 0 = its start; from its start = before the start.
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Section, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Section, "-1")),
        (p0, 0, 1, Some(Clamp::ToFirst))
    );
    // -1 from the start of section 2 goes to section 1; -1 from inside section 2 to its own start.
    assert_eq!(
        at(&reach(&f, (p4, 0), GotoKind::Section, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (f.t2c, 0), GotoKind::Section, "-1")),
        (p4, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (f.t2c, 0), GotoKind::Section, "-2")),
        (p0, 0, 1, None)
    );
    // +0 is the current one.
    assert_eq!(
        at(&reach(&f, (p3, 4), GotoKind::Section, "+0")),
        (p0, 0, 1, None)
    );
    // Out of range.
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Section, "99")),
        (p4, 0, 2, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Section, "0")),
        (p0, 0, 1, Some(Clamp::ToFirst))
    );
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Section, "-9")),
        (p0, 0, 1, Some(Clamp::ToFirst))
    );
    let l = reach(&f, (p1, 0), GotoKind::Section, "2");
    assert_eq!((l.zone, l.section, l.count), (Zone::Body, 1, 2));
}

#[test]
fn tables() {
    let f = fixture();
    let [p0, p1, _, p3, ..] = f.p;
    let [c00, _, _, c11] = f.c;
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Table, "")),
        (c00, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Table, "2")),
        (f.t2c, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Table, "+2")),
        (f.t2c, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Table, "+3")),
        (f.t2c, 0, 2, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Table, "-1")),
        (c00, 0, 1, Some(Clamp::ToFirst))
    );
    // From inside table 1: next is table 2, previous is the start of table 1.
    assert_eq!(
        at(&reach(&f, (c11, 0), GotoKind::Table, "")),
        (f.t2c, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (c11, 0), GotoKind::Table, "-1")),
        (c00, 0, 1, None)
    );
    // Between the tables, after table 1.
    assert_eq!(
        at(&reach(&f, (p3, 3), GotoKind::Table, "-1")),
        (c00, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 3), GotoKind::Table, "+1")),
        (f.t2c, 0, 2, None)
    );
    // Before table 1.
    assert_eq!(
        at(&reach(&f, (p1, 1), GotoKind::Table, "+1")),
        (c00, 0, 1, None)
    );
    let l = reach(&f, (p0, 0), GotoKind::Table, "2");
    assert_eq!((l.zone, l.section), (Zone::Table, 1));
}

#[test]
fn objects_and_headings() {
    let f = fixture();
    let [p0, p1, p2, p3, p4, _] = f.p;
    // Objects: p2@2, p3@0.
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Object, "1")),
        (p2, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Object, "2")),
        (p3, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Object, "")),
        (p2, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p2, 2), GotoKind::Object, "")),
        (p3, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p2, 1), GotoKind::Object, "")),
        (p2, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 0), GotoKind::Object, "-1")),
        (p2, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 5), GotoKind::Object, "+1")),
        (p3, 0, 2, Some(Clamp::ToLast))
    );
    // Headings: p0, p3, p4 (styles atelier.heading.1/2 and Heading1); p1 is not one.
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Heading, "")),
        (p3, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Heading, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Heading, "3")),
        (p4, 0, 3, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Heading, "4")),
        (p4, 0, 3, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p3, 4), GotoKind::Heading, "-1")),
        (p3, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 4), GotoKind::Heading, "-2")),
        (p0, 0, 1, None)
    );
}

#[test]
fn bookmarks() {
    let mut f = fixture();
    let [p0, p1, _, p3, ..] = f.p;
    bookmark_at(&mut f.doc, "alpha", p3, 1, 4);
    bookmark_at(&mut f.doc, "beta", p0, 2, 2);
    bookmark_at(&mut f.doc, "_hid", p1, 0, 1);
    // Position order of visible ones: beta (p0@2), alpha (p3@1..4).
    let l = reach(&f, (p0, 0), GotoKind::Bookmark, "ALPHA");
    assert_eq!(at(&l), (p3, 1, 2, None));
    assert_eq!(l.selection_end.map(|e| (e.node, e.offset)), Some((p3, 4)));
    assert_eq!((l.zone, l.count), (Zone::Body, 2));
    let l = reach(&f, (p3, 0), GotoKind::Bookmark, " beta ");
    assert_eq!(at(&l), (p0, 2, 1, None));
    assert_eq!(l.selection_end, None);
    // Hidden ones are reachable by name only.
    let l = reach(&f, (p0, 0), GotoKind::Bookmark, "_hid");
    assert_eq!((l.position.node, l.index), (p1, 0));
    // Next / previous / numbers skip them.
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Bookmark, "")),
        (p0, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 2), GotoKind::Bookmark, "")),
        (p3, 1, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 1), GotoKind::Bookmark, "-1")),
        (p0, 2, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 9), GotoKind::Bookmark, "-1")),
        (p3, 1, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p3, 9), GotoKind::Bookmark, "+1")),
        (p3, 1, 2, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Bookmark, "2")),
        (p3, 1, 2, None)
    );
    assert_eq!(
        goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Bookmark, "zzz"),
        Err(GotoError::BookmarkNotFound("zzz".into()))
    );
    assert_eq!(
        goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Bookmark, "+x"),
        Err(GotoError::Parse(ParseError::InvalidNumber))
    );
    // Zone of a bookmark in a header.
    let section = first_section(&f.doc);
    let hdr = add_header(&mut f.doc, section, "head");
    bookmark_at(&mut f.doc, "inhdr", hdr, 0, 2);
    let l = reach(&f, (p0, 0), GotoKind::Bookmark, "inhdr");
    assert_eq!((l.zone, l.section, l.position.node), (Zone::Header, 0, hdr));
}

#[test]
fn pages_and_lines() {
    let f = fixture();
    let [p0, p1, p2, p3, p4, p5] = f.p;
    let [c00, _, c10, _] = f.c;
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "3")),
        (p3, 0, 3, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "9")),
        (p5, 0, 5, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "0")),
        (p0, 0, 1, Some(Clamp::ToFirst))
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Page, "+1")),
        (c10, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Page, "")),
        (c10, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Page, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (c10, 0), GotoKind::Page, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p2, 3), GotoKind::Page, "+2")),
        (p4, 0, 4, None)
    );
    assert_eq!(
        at(&reach(&f, (p5, 0), GotoKind::Page, "+1")),
        (p5, 0, 5, Some(Clamp::ToLast))
    );
    // Compound: pages 1-3 are section 1, pages 4-5 section 2.
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p2s1")),
        (c10, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p9s1")),
        (p3, 0, 3, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p1s2")),
        (p4, 0, 4, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "P2 S2")),
        (p5, 0, 5, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p3s2")),
        (p5, 0, 5, Some(Clamp::ToLast))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p0s1")),
        (p0, 0, 1, Some(Clamp::ToFirst))
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Page, "p1s7")),
        (p4, 0, 4, Some(Clamp::ToLast))
    );
    // Lines: p0, p1, c00, p2, p3, p4.
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Line, "4")),
        (p2, 0, 4, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Line, "+1")),
        (c00, 0, 3, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 2), GotoKind::Line, "-1")),
        (p1, 0, 2, None)
    );
    assert_eq!(
        at(&reach(&f, (p1, 0), GotoKind::Line, "-1")),
        (p0, 0, 1, None)
    );
    assert_eq!(
        at(&reach(&f, (p0, 0), GotoKind::Line, "7")),
        (p4, 0, 6, Some(Clamp::ToLast))
    );
    // After the last page start, inside the last line: +1 clamps.
    assert_eq!(
        at(&reach(&f, (f.t2c, 0), GotoKind::Line, "+1")),
        (p4, 0, 6, Some(Clamp::ToLast))
    );
    let l = reach(&f, (p0, 0), GotoKind::Page, "4");
    assert_eq!((l.zone, l.section, l.count), (Zone::Body, 1, 5));
}

#[test]
fn kinds_without_elements() {
    let (doc, p0) = with_text("plain");
    let map = FixedPageMap::new(&doc, vec![], vec![]);
    for kind in [
        GotoKind::Page,
        GotoKind::Line,
        GotoKind::Bookmark,
        GotoKind::Table,
        GotoKind::Object,
        GotoKind::Heading,
    ] {
        for input in ["", "1", "+2", "-1", "0", "99"] {
            assert_eq!(
                goto(&doc, &map, pos(p0, 0), kind, input),
                Ok(GotoOutcome::NoElements),
                "{kind:?} {input:?}"
            );
        }
        for d in [Direction::Next, Direction::Previous] {
            assert_eq!(
                step(&doc, &map, pos(p0, 0), kind, d),
                Ok(GotoOutcome::NoElements)
            );
        }
    }
    assert_eq!(
        goto(&doc, &map, pos(p0, 0), GotoKind::Page, "p1s1"),
        Ok(GotoOutcome::NoElements)
    );
    // The only section is always there.
    let GotoOutcome::Reached(l) = goto(&doc, &map, pos(p0, 2), GotoKind::Section, "").unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (l.position.node, l.index, l.clamp),
        (p0, 1, Some(Clamp::ToLast))
    );
}

#[test]
fn step_buttons_match_relative_amounts() {
    let f = fixture();
    let [p0, p1, ..] = f.p;
    for kind in [
        GotoKind::Page,
        GotoKind::Line,
        GotoKind::Section,
        GotoKind::Table,
        GotoKind::Object,
        GotoKind::Heading,
    ] {
        for from in [(p0, 0), (p1, 2), (f.c[3], 0), (f.t2c, 0)] {
            for (d, s) in [(Direction::Next, "+1"), (Direction::Previous, "-1")] {
                assert_eq!(
                    step(&f.doc, &f.map, pos(from.0, from.1), kind, d),
                    goto(&f.doc, &f.map, pos(from.0, from.1), kind, s),
                    "{kind:?} {from:?} {s}"
                );
            }
        }
    }
}

#[test]
fn unknown_position_and_arbitrary_input() {
    let f = fixture();
    let ghost = f.doc.clone().alloc_id().unwrap();
    assert_eq!(
        goto(&f.doc, &f.map, pos(ghost, 0), GotoKind::Table, "1"),
        Err(GotoError::UnknownPosition)
    );
    // No panic whatever the input.
    let junk = [
        "\0",
        "💥",
        "+\u{301}",
        "9".repeat(400).leak(),
        "p\u{0}s",
        "-",
        "  \t\n",
        "ｐ３ｓ２",
        "é",
        "_",
        "p9s9s9",
    ];
    for kind in [
        GotoKind::Page,
        GotoKind::Line,
        GotoKind::Section,
        GotoKind::Bookmark,
        GotoKind::Table,
        GotoKind::Object,
        GotoKind::Heading,
    ] {
        for input in junk {
            let _ = goto(&f.doc, &f.map, pos(f.p[0], 0), kind, input);
        }
    }
}

fn reached(r: Result<GotoOutcome, GotoError>) -> Landing {
    match r.unwrap() {
        GotoOutcome::Reached(l) => l,
        GotoOutcome::NoElements => panic!("no element"),
    }
}

/// Goes to element 1, then Next up to the last (with the rank of each result
/// as the origin of the next request), then once more; then the same backwards.
/// Every element is visited once, and a clamp is announced only at a real end.
fn walk(
    doc: &RichDocument,
    map: &dyn PageMap,
    first: TextPosition,
    kind: GotoKind,
    styles: Option<&StyleCatalog>,
) -> usize {
    let go = |from: GotoOrigin, d| match styles {
        Some(s) => step_with_styles(doc, map, from, kind, d, s),
        None => step(doc, map, from, kind, d),
    };
    let mut l = reached(match styles {
        Some(s) => goto_with_styles(doc, map, first, kind, "1", s),
        None => goto(doc, map, first, kind, "1"),
    });
    let n = l.count;
    assert_eq!((l.index, l.clamp), (1, None), "{kind:?}");
    for i in 2..=n {
        l = reached(go(l.origin(), Direction::Next));
        assert_eq!((l.index, l.clamp), (i, None), "{kind:?} next {i}/{n}");
    }
    let over = reached(go(l.origin(), Direction::Next));
    assert_eq!(
        (over.index, over.clamp),
        (n, Some(Clamp::ToLast)),
        "{kind:?} past the end"
    );
    for i in (1..n).rev() {
        l = reached(go(l.origin(), Direction::Previous));
        assert_eq!((l.index, l.clamp), (i, None), "{kind:?} previous {i}/{n}");
    }
    let under = reached(go(l.origin(), Direction::Previous));
    assert_eq!(
        (under.index, under.clamp),
        (1, Some(Clamp::ToFirst)),
        "{kind:?} before the start"
    );
    n
}

#[test]
fn repeated_next_and_previous_visit_every_element() {
    let mut f = fixture();
    let [p0, p1, _, p3, ..] = f.p;
    bookmark_at(&mut f.doc, "alpha", p3, 1, 4);
    bookmark_at(&mut f.doc, "beta", p0, 2, 2);
    bookmark_at(&mut f.doc, "gamma", p1, 0, 1);
    let counts: Vec<usize> = [
        GotoKind::Page,
        GotoKind::Line,
        GotoKind::Section,
        GotoKind::Table,
        GotoKind::Object,
        GotoKind::Heading,
        GotoKind::Bookmark,
    ]
    .iter()
    .map(|k| walk(&f.doc, &f.map, pos(p0, 0), *k, None))
    .collect();
    assert_eq!(counts, [5, 6, 2, 2, 2, 3, 3]);
}

/// Elements that start at the same position: a table nested at the start of
/// the first cell of another, objects at the same offset, bookmarks at the
/// same place.
fn coincident() -> (RichDocument, FixedPageMap, NodeId, NodeId, NodeId) {
    let (mut doc, p0) = with_text("intro text");
    let section = first_section(&doc);
    add_object(&mut doc, p0, 2);
    add_object(&mut doc, p0, 2);
    add_object(&mut doc, p0, 2);
    bookmark_at(&mut doc, "ba", p0, 1, 1);
    bookmark_at(&mut doc, "bb", p0, 1, 3);
    let (nested, outer) = add_nested_table(&mut doc, section);
    let map = FixedPageMap::new(&doc, vec![pos(p0, 0)], vec![pos(p0, 0)]);
    (doc, map, p0, nested, outer)
}

#[test]
fn coincident_starts_are_stepped_one_by_one() {
    let (doc, map, p0, nested, outer) = coincident();
    for (kind, n) in [
        (GotoKind::Table, 2),
        (GotoKind::Object, 3),
        (GotoKind::Bookmark, 2),
        (GotoKind::Section, 1),
        (GotoKind::Page, 1),
    ] {
        assert_eq!(walk(&doc, &map, pos(p0, 0), kind, None), n, "{kind:?}");
    }
    // The nested table and its parent both start in the nested cell.
    let first = reached(goto(&doc, &map, pos(p0, 0), GotoKind::Table, "1"));
    let second = reached(step(
        &doc,
        &map,
        first.origin(),
        GotoKind::Table,
        Direction::Next,
    ));
    assert_eq!(
        (first.position, second.position),
        (pos(nested, 0), pos(nested, 0))
    );
    assert_eq!((second.index, second.clamp, second.count), (2, None, 2));
    // Without the rank the caret counts as on the last of them: a real end.
    let blind = reached(step(
        &doc,
        &map,
        pos(nested, 0),
        GotoKind::Table,
        Direction::Next,
    ));
    assert_eq!((blind.index, blind.clamp), (2, Some(Clamp::ToLast)));
    // A stale rank (the caret moved) is ignored.
    let stale = GotoOrigin {
        position: pos(outer, 0),
        rank: Some(1),
    };
    let l = reached(step(
        &doc,
        &map,
        stale,
        GotoKind::Table,
        Direction::Previous,
    ));
    assert_eq!((l.index, l.position), (2, pos(nested, 0)));
}

#[test]
fn bookmark_names_beat_numbers_and_exact_beats_case() {
    let mut f = fixture();
    let [p0, p1, _, p3, ..] = f.p;
    raw_bookmark(&mut f.doc, "1abc", p1, 0, 1);
    raw_bookmark(&mut f.doc, "12", p3, 2, 2);
    raw_bookmark(&mut f.doc, "Titre", p0, 0, 2);
    raw_bookmark(&mut f.doc, "titre", p0, 4, 6);
    let go = |input: &str| reached(goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Bookmark, input));
    assert_eq!(go("1abc").position, pos(p1, 0));
    assert_eq!(go(" 12 ").position, pos(p3, 2));
    // "1" is no bookmark: absolute rank 1 in position order (Titre at p0@0).
    assert_eq!(go("1").position, pos(p0, 0));
    let l = go("titre");
    assert_eq!((l.position, l.ambiguous), (pos(p0, 4), false));
    let l = go("Titre");
    assert_eq!((l.position, l.ambiguous), (pos(p0, 0), false));
    let l = go("TITRE");
    assert_eq!((l.position, l.ambiguous), (pos(p0, 0), true));
    // Still an error when nothing matches.
    assert_eq!(
        goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Bookmark, "1xyz"),
        Err(GotoError::Parse(ParseError::InvalidNumber))
    );
    assert_eq!(
        goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Bookmark, "xyz"),
        Err(GotoError::BookmarkNotFound("xyz".into()))
    );
}

#[test]
fn inherited_headings_with_a_style_catalogue() {
    let mut f = fixture();
    let [_, p1, p2, ..] = f.p;
    let mut styles = default_rich_document().styles;
    let based = |id: &str, on: &str| Style {
        id: id.into(),
        kind: StyleKind::Paragraph,
        based_on: Some(on.into()),
        ..Style::default()
    };
    styles.set_style(based("Sub", "atelier.heading.2")).unwrap();
    styles.set_style(based("Sub2", "Sub")).unwrap();
    styles.set_style(based("LoopA", "LoopB")).unwrap();
    styles.set_style(based("LoopB", "LoopA")).unwrap();
    // A chain longer than the bound, ending in a heading, is not followed to the end.
    styles
        .set_style(based("Deep69", "atelier.heading.1"))
        .unwrap();
    for i in (0..69).rev() {
        styles
            .set_style(based(&format!("Deep{i}"), &format!("Deep{}", i + 1)))
            .unwrap();
    }
    let catalog = styles.catalog();
    assert!(is_heading_style_in("atelier.heading.3", catalog));
    assert!(is_heading_style_in("Sub", catalog) && is_heading_style_in("Sub2", catalog));
    assert!(!is_heading_style_in("LoopA", catalog));
    assert!(!is_heading_style_in("Unknown", catalog));
    assert!(!is_heading_style_in("atelier.normal", catalog));
    assert!(is_heading_style_in("Deep30", catalog) && !is_heading_style_in("Deep0", catalog));

    set_style(&mut f.doc, f.p[1], "Sub2");
    set_style(&mut f.doc, p2, "LoopA");
    let p0 = f.p[0];
    // Headings: p0, p3, p4 directly; p1 only through the catalogue.
    let plain = reached(goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Heading, "1"));
    assert_eq!(plain.count, 3);
    let with = reached(goto_with_styles(
        &f.doc,
        &f.map,
        pos(p0, 0),
        GotoKind::Heading,
        "2",
        catalog,
    ));
    assert_eq!((with.count, with.position), (4, pos(p1, 0)));
    walk(&f.doc, &f.map, pos(p0, 0), GotoKind::Heading, Some(catalog));
}

#[test]
fn extra_page_forms_and_positions_in_a_header() {
    let f = fixture();
    let [p0, ..] = f.p;
    let go = |input: &str| reached(goto(&f.doc, &f.map, pos(p0, 0), GotoKind::Page, input));
    assert_eq!(at(&go("P3 S1")), (f.p[3], 0, 3, None));
    assert_eq!(at(&go("p2s0")), (f.c[2], 0, 2, Some(Clamp::ToFirst)));
    assert_eq!(at(&go("p1s99")), (f.p[4], 0, 4, Some(Clamp::ToLast)));
    // No page at all.
    let empty = FixedPageMap::new(&f.doc, vec![], vec![]);
    for input in ["p0s1", "P3 S2", "p1s9", "1", "+1", ""] {
        assert_eq!(
            goto(&f.doc, &empty, pos(p0, 0), GotoKind::Page, input),
            Ok(GotoOutcome::NoElements),
            "{input:?}"
        );
    }
    // The caret in a header precedes the whole body.
    let (mut doc, a) = with_text("a");
    let s0 = first_section(&doc);
    let hp = add_header(&mut doc, s0, "head");
    let (_, b) = add_section(&mut doc);
    let t = add_table_to(&mut doc, s0, 1, 1);
    let tp = cell_para(&doc, cells(&doc, t)[0][0]);
    let map = FixedPageMap::new(&doc, vec![pos(a, 0), pos(b, 0)], vec![]);
    let go = |kind, input: &str| reached(goto(&doc, &map, pos(hp, 1), kind, input));
    assert_eq!(at(&go(GotoKind::Section, "")), (a, 0, 1, None));
    assert_eq!(
        at(&go(GotoKind::Section, "-1")),
        (a, 0, 1, Some(Clamp::ToFirst))
    );
    assert_eq!(at(&go(GotoKind::Table, "")), (tp, 0, 1, None));
    assert_eq!(at(&go(GotoKind::Page, "")), (a, 0, 1, None));
    assert_eq!(at(&go(GotoKind::Page, "p1s2")), (b, 0, 2, None));
    assert_eq!(go(GotoKind::Table, "1").zone, Zone::Table);
}

/// A page map that contradicts itself, from a seeded generator.
struct Chaos<'a> {
    rng: std::cell::RefCell<u64>,
    nodes: &'a [NodeId],
}

impl Chaos<'_> {
    fn next(&self, n: usize) -> usize {
        let mut s = self.rng.borrow_mut();
        *s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*s >> 33) % n as u64) as usize
    }
    fn count(&self) -> usize {
        [0, 1, 2, 5, usize::MAX][self.next(5)]
    }
    fn start(&self) -> Option<TextPosition> {
        match self.next(4) {
            0 => None,
            1 => Some(pos(self.nodes[0], usize::MAX)),
            _ => Some(pos(self.nodes[self.next(self.nodes.len())], self.next(3))),
        }
    }
    fn rank(&self) -> usize {
        [0, 1, 3, 5, usize::MAX][self.next(5)]
    }
}

impl PageMap for Chaos<'_> {
    fn page_count(&self) -> usize {
        self.count()
    }
    fn page_start(&self, _: usize) -> Option<TextPosition> {
        self.start()
    }
    fn page_of(&self, _: TextPosition) -> usize {
        self.rank()
    }
    fn line_count(&self) -> usize {
        self.count()
    }
    fn line_start(&self, _: usize) -> Option<TextPosition> {
        self.start()
    }
    fn line_of(&self, _: TextPosition) -> usize {
        self.rank()
    }
}

/// One page, yet the position is said to be on page 5.
struct Single(TextPosition);
impl PageMap for Single {
    fn page_count(&self) -> usize {
        1
    }
    fn page_start(&self, n: usize) -> Option<TextPosition> {
        (n == 1).then_some(self.0)
    }
    fn page_of(&self, _: TextPosition) -> usize {
        5
    }
    fn line_count(&self) -> usize {
        0
    }
    fn line_start(&self, _: usize) -> Option<TextPosition> {
        None
    }
    fn line_of(&self, _: TextPosition) -> usize {
        0
    }
}

/// Three pages, none of which has a start.
struct Hole;
impl PageMap for Hole {
    fn page_count(&self) -> usize {
        3
    }
    fn page_start(&self, _: usize) -> Option<TextPosition> {
        None
    }
    fn page_of(&self, _: TextPosition) -> usize {
        2
    }
    fn line_count(&self) -> usize {
        0
    }
    fn line_start(&self, _: usize) -> Option<TextPosition> {
        None
    }
    fn line_of(&self, _: TextPosition) -> usize {
        0
    }
}

#[test]
fn inconsistent_page_maps_never_panic() {
    let f = fixture();
    let ghost = f.doc.clone().alloc_id().unwrap();
    let mut nodes = f.p.to_vec();
    nodes.extend(f.c);
    nodes.push(f.t2c);
    nodes.push(ghost);
    let from = pos(f.p[1], 0);
    let l = reached(goto(
        &f.doc,
        &Single(pos(f.p[0], 0)),
        from,
        GotoKind::Page,
        "p1s1",
    ));
    assert_eq!((l.index, l.position), (1, pos(f.p[0], 0)));
    assert_eq!(
        goto(&f.doc, &Single(pos(ghost, 0)), from, GotoKind::Page, "p1s1"),
        Err(GotoError::InconsistentPageMap)
    );
    for input in ["2", "+1", "p1s1"] {
        assert_eq!(
            goto(&f.doc, &Hole, from, GotoKind::Page, input),
            Err(GotoError::InconsistentPageMap),
            "{input}"
        );
    }
    let inputs = [
        "",
        "1",
        "0",
        "+1",
        "-1",
        "+1000000000",
        "-1000000000",
        "1000000000",
        "p1s1",
        "p3s2",
        "p0s0",
        "p99s99",
        "P1000000000S1000000000",
        "+0",
    ];
    for seed in 0..40u64 {
        let chaos = Chaos {
            rng: std::cell::RefCell::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15)),
            nodes: &nodes,
        };
        for i in 0..200 {
            let kind = [GotoKind::Page, GotoKind::Line][chaos.next(2)];
            let from = pos(nodes[chaos.next(nodes.len())], chaos.next(4));
            let input = inputs[(i + chaos.next(inputs.len())) % inputs.len()];
            let _ = goto(&f.doc, &chaos, from, kind, input);
            let d = [Direction::Next, Direction::Previous][chaos.next(2)];
            let origin = GotoOrigin {
                position: from,
                rank: Some(chaos.next(7)),
            };
            let _ = step(&f.doc, &chaos, origin, kind, d);
        }
    }
}
