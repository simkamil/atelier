// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property test of the bookmark commands (US-291): fixed-seed random runs of
//! insertions, moves, renames, deletions and undos. After every step the
//! model invariants hold, names are unique ignoring case, the position list
//! follows the reading order, and every undo restores the exact document.

mod common;
mod nav_common;

use atelier_model::Transaction;
use atelier_text_model::navigation::*;
use atelier_text_model::rich::*;
use atelier_text_model::search::ParagraphSource;
use common::*;
use nav_common::*;
use std::collections::BTreeSet;

const SEEDS: u64 = 48;
const STEPS: usize = 120;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5851_f42d_4c95_7f2d)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

const NAMES: &[&str] = &[
    "Alpha", "alpha", "ALPHA", "beta", "Béta", "gamma_2", "Delta", "é1", "_Toc1", "_GoBack",
    "1bad", "a b", "",
];

fn check(doc: &RichDocument) {
    doc.validate().unwrap();
    let all = list_bookmarks(doc, BookmarkOrder::Position, true);
    // Unique ignoring case.
    let keys: BTreeSet<String> = all.iter().map(|b| name_key(&b.name)).collect();
    assert_eq!(keys.len(), all.len(), "duplicate names: {all:?}");
    // Reading order.
    let reading = doc.paragraphs();
    let mut last = (0, 0, 0);
    for b in &all {
        assert!(b.start <= b.end);
        assert_eq!(reading[b.paragraph_index].id, b.paragraph);
        assert_eq!(reading[b.paragraph_index].zone, b.zone);
        let key = (b.paragraph_index, b.start, b.end);
        assert!(key >= last, "position order broken at {}", b.name);
        last = key;
    }
    // Visible list: same items minus hidden, in both orders; name order sorted.
    let visible = list_bookmarks(doc, BookmarkOrder::Position, false);
    assert_eq!(visible.len(), all.iter().filter(|b| !b.hidden).count());
    let by_name = list_bookmarks(doc, BookmarkOrder::Name, false);
    let mut ids: Vec<_> = by_name.iter().map(|b| b.id).collect();
    ids.sort();
    let mut vids: Vec<_> = visible.iter().map(|b| b.id).collect();
    vids.sort();
    assert_eq!(ids, vids);
    let lower: Vec<String> = by_name.iter().map(|b| b.name.to_lowercase()).collect();
    for pair in by_name.windows(2) {
        // Names of the pool are ASCII or single accented letters: plain
        // lowercase order agrees with the folded order except accents, which
        // sort next to their base letter; check the unaccented ones only.
        if pair[0].name.is_ascii() && pair[1].name.is_ascii() {
            assert!(
                pair[0].name.to_lowercase() <= pair[1].name.to_lowercase(),
                "{lower:?}"
            );
        }
    }
    // Every name is reachable by Go To at its start.
    let map = FixedPageMap::new(doc, vec![], vec![]);
    for b in &all {
        let from = pos(reading[0].id, 0);
        let GotoOutcome::Reached(l) = goto(doc, &map, from, GotoKind::Bookmark, &b.name).unwrap()
        else {
            panic!("{} unreachable", b.name);
        };
        assert_eq!((l.position.node, l.position.offset), (b.paragraph, b.start));
    }
}

fn links_to(doc: &RichDocument, name: &str) -> usize {
    let key = name_key(name);
    doc.paragraph_ids()
        .iter()
        .map(|p| {
            doc.paragraph(*p)
                .unwrap()
                .links
                .iter()
                .filter(|l| matches!(&l.target, LinkTarget::Bookmark(n) if name_key(n) == key))
                .count()
        })
        .sum()
}

#[test]
fn random_bookmark_sessions() {
    let mut applied = 0;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        let (mut doc, p0) = with_text("abcdefghij");
        let section = first_section(&doc);
        let p1 = add_para(&mut doc, section, "klmnopqrst");
        let table = add_table_to(&mut doc, section, 1, 2);
        let cp = cell_para(&doc, cells(&doc, table)[0][0]);
        set_text(&mut doc, cp, "uvwxyz");
        let hdr = add_header(&mut doc, section, "header text");
        add_link(&mut doc, p0, 0, 3, "alpha");
        add_link(&mut doc, p1, 0, 2, "ALPHA");
        add_link(&mut doc, p1, 4, 6, "beta");
        let paras = [p0, p1, cp, hdr];
        let mut undo: Vec<(Transaction<RichOp>, RichDocument, usize)> = Vec::new();
        check(&doc);
        for _ in 0..STEPS {
            let before = doc.clone();
            let name = NAMES[rng.below(NAMES.len())];
            let edit = match rng.below(5) {
                0 | 1 => {
                    let p = paras[rng.below(paras.len())];
                    let len = doc.paragraph(p).unwrap().text.len();
                    let (a, b) = (rng.below(len + 1), rng.below(len + 1));
                    let mode = if rng.below(2) == 0 {
                        NameMode::User
                    } else {
                        NameMode::Internal
                    };
                    insert_bookmark(&mut doc, name, pos(p, a), pos(p, b), mode).map(|e| (e, None))
                }
                2 => {
                    let old = NAMES[rng.below(NAMES.len())];
                    let mode = if rng.below(2) == 0 {
                        NameMode::User
                    } else {
                        NameMode::Internal
                    };
                    rename_bookmark(&doc, old, name, mode).map(|e| (e, Some((old, name))))
                }
                3 => delete_bookmark(&doc, name).map(|e| (e, None)),
                _ => {
                    // Undo the last successful edit.
                    if let Some((inverse, snapshot, _)) = undo.pop() {
                        doc.apply(&inverse).unwrap();
                        assert_eq!(doc, snapshot, "seed {seed}: undo");
                        check(&doc);
                    }
                    continue;
                }
            };
            match edit {
                Err(_) => assert_eq!(
                    doc, before,
                    "seed {seed}: refused edit changed the document"
                ),
                Ok((edit, renamed_to)) => {
                    let inverse = doc.apply(&edit.transaction).unwrap();
                    if let Some((old, new)) = renamed_to {
                        // Every link that pointed to the old name follows the rename.
                        let moved = if name_key(old) == name_key(new) {
                            0
                        } else {
                            edit.links.len()
                        };
                        assert_eq!(
                            links_to(&doc, new),
                            links_to(&before, new) + moved,
                            "seed {seed}"
                        );
                    }
                    applied += 1;
                    undo.push((inverse, before, 0));
                    check(&doc);
                }
            }
        }
        // Unwind everything: back to the starting document.
        while let Some((inverse, snapshot, _)) = undo.pop() {
            doc.apply(&inverse).unwrap();
            assert_eq!(doc, snapshot, "seed {seed}: final unwind");
        }
        check(&doc);
    }
    assert!(applied > 500, "only {applied} edits applied");
}
