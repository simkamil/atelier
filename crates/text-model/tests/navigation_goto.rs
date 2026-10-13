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
