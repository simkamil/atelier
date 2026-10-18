// SPDX-License-Identifier: AGPL-3.0-or-later
//! The prefix-sum index of `RichDocument` against the test double
//! `VecParagraphs` (linear, built from the whole flat text), on random
//! documents with objects, breaks, symbols, tables and opaque blocks (US-204).
#![cfg(not(target_arch = "wasm32"))]

mod flat_common;

use atelier_text_model::rich::flat::{BlockKind, FlatError, FlatIndex, LayoutContext};
use atelier_text_model::rich::flat_double::VecParagraphs;
use atelier_text_model::rich::{Node, RichDocument};
use flat_common::*;

fn para_at(doc: &RichDocument, index: usize) -> Option<&atelier_text_model::rich::Paragraph> {
    LayoutContext::paragraph(doc, index)
}

fn reference(doc: &RichDocument) -> VecParagraphs {
    let mut entries = Vec::new();
    let mut kinds = Vec::new();
    for i in 0..doc.paragraph_count() {
        kinds.push(doc.block_kind(i).unwrap());
        entries.push(para_at(doc, i).cloned().unwrap_or_default());
    }
    let mut reference = VecParagraphs::new(entries);
    for (i, kind) in kinds.into_iter().enumerate() {
        if kind != BlockKind::Body {
            reference.set_kind(i, kind);
        }
    }
    reference
}

fn compare(doc: &RichDocument) {
    let reference = reference(doc);
    assert_eq!(doc.len(), reference.len());
    assert_eq!(doc.paragraph_count(), reference.paragraph_count());
    assert_eq!(
        doc.text(0, doc.len()).unwrap(),
        reference.text(0, reference.len()).unwrap()
    );
    for i in 0..doc.paragraph_count() {
        assert_eq!(doc.paragraph_start(i), reference.paragraph_start(i));
        assert_eq!(doc.paragraph_flat(i), reference.paragraph_flat(i));
        assert_eq!(doc.block_kind(i), reference.block_kind(i));
    }
    let len = doc.len();
    for p in 0..=len + 1 {
        assert_eq!(doc.locate(p), reference.locate(p), "locate {p}");
        assert_eq!(
            doc.check_position(p),
            reference.check_position(p),
            "check {p}"
        );
        assert_eq!(doc.snap_back(p), reference.snap_back(p), "snap {p}");
        assert_eq!(doc.prev_grapheme(p), reference.prev_grapheme(p), "pg {p}");
        assert_eq!(doc.next_grapheme(p), reference.next_grapheme(p), "ng {p}");
        assert_eq!(doc.prev_word(p), reference.prev_word(p), "pw {p}");
        assert_eq!(doc.next_word(p), reference.next_word(p), "nw {p}");
        assert_eq!(doc.word_at(p), reference.word_at(p), "wa {p}");
        if let Ok(at) = doc.locate(p) {
            assert_eq!(doc.position(at), Ok(p));
            assert_eq!(doc.position(at), reference.position(at));
        }
    }
    let valid: Vec<usize> = (0..=len)
        .filter(|p| doc.check_position(*p).is_ok())
        .collect();
    for window in valid.windows(2) {
        assert_eq!(
            doc.text(window[0], window[1]),
            reference.text(window[0], window[1])
        );
    }
    if len > 0 {
        assert_eq!(doc.text(len, 0), Err(FlatError::InvalidRange));
    }
}

#[test]
fn index_matches_the_double_on_random_bodies() {
    for seed in 0..120 {
        let mut rng = Rng::new(seed);
        let count = 1 + rng.below(5);
        let doc = random_body(&mut rng, count, true);
        compare(&doc);
    }
}

#[test]
fn index_matches_the_double_with_tables_and_opaque_blocks() {
    for seed in 0..120 {
        let mut rng = Rng::new(1000 + seed);
        let count = 1 + rng.below(4);
        let doc = random_structured(&mut rng, count);
        compare(&doc);
        // The order of the index is the document order of the paragraphs.
        let ids = doc.paragraph_ids();
        let mut next = 0;
        for i in 0..doc.paragraph_count() {
            if let Some(p) = para_at(&doc, i) {
                let Some(Node::Paragraph(expected)) = doc.node(ids[next]) else {
                    panic!("not a paragraph");
                };
                assert_eq!(p, expected);
                next += 1;
            }
        }
        assert_eq!(next, ids.len());
    }
}

#[test]
fn positions_inside_objects_and_clusters_are_refused() {
    // An object after "e": positions 2 and 3 are inside U+FFFC.
    let mut doc = RichDocument::new();
    let first = doc.paragraph_ids()[0];
    apply(
        &mut doc,
        vec![atelier_text_model::rich::RichOp::InsertText {
            para: first,
            offset: 0,
            text: "e".into(),
        }],
    );
    add_item_at(&mut doc, first, 1);
    // "e" + U+FFFC
    assert_eq!(doc.len(), 4);
    assert_eq!(doc.check_position(1), Ok(()));
    assert_eq!(doc.check_position(2), Err(FlatError::NotBoundary));
    assert_eq!(doc.check_position(3), Err(FlatError::NotBoundary));
    assert_eq!(doc.check_position(4), Ok(()));
    assert_eq!(doc.check_position(5), Err(FlatError::OutOfRange));
    assert_eq!(doc.snap_back(3), Ok(1));
    assert_eq!(doc.next_grapheme(1), Ok(Some(4)));
    assert_eq!(doc.prev_grapheme(4), Ok(Some(1)));
}

fn add_item_at(doc: &mut RichDocument, para: atelier_text_model::rich::NodeId, offset: usize) {
    use atelier_text_model::rich::{Inline, InlineSymbol, RichOp};
    let id = doc.alloc_id().unwrap();
    apply(
        doc,
        vec![RichOp::InsertInline {
            para,
            item: Inline::Symbol(InlineSymbol {
                id,
                offset,
                font: "Symbol".into(),
                code: 0xf0b7,
            }),
        }],
    );
}

#[test]
fn layout_context_serves_sections_styles_pages_resources_and_counters() {
    use atelier_text_model::rich::flat::PageSetup;
    use atelier_text_model::rich::{DocumentStyleDefaults, ListRef, RichOp, StyleCatalog, Theme};
    let mut rng = Rng::new(3);
    let mut doc = random_structured(&mut rng, 3);
    // Page setup from the section attributes.
    assert_eq!(doc.page_setup(0), Ok(PageSetup::standard()));
    assert_eq!(doc.page_setup(1), Err(FlatError::OutOfRange));
    let (index, s) = doc.section_of(0).unwrap();
    assert_eq!((index, s.opaque.len()), (0, 0));
    assert_eq!(
        doc.section_of(doc.paragraph_count()),
        Err(FlatError::OutOfRange)
    );
    // Resolved properties follow the catalog.
    let mut defaults = DocumentStyleDefaults::default();
    defaults.character.values.font = Some("Carlito".into());
    defaults.paragraph.space_after = Some(160);
    doc.set_styles(StyleCatalog::new(defaults), Theme::default());
    let first_body = (0..doc.paragraph_count())
        .find(|i| doc.block_kind(*i) == Ok(BlockKind::Body))
        .unwrap();
    let resolved = doc.resolved(first_body).unwrap();
    assert_eq!(resolved.paragraph.space_after, Some(160));
    assert_eq!(resolved.character.font.as_deref(), Some("Carlito"));
    let opaque = (0..doc.paragraph_count())
        .find(|i| doc.block_kind(*i) == Ok(BlockKind::Opaque))
        .unwrap();
    assert_eq!(doc.resolved(opaque).unwrap(), Default::default());
    assert!(para_at(&doc, opaque).is_none());
    assert_eq!(doc.paragraph_flat(opaque).unwrap(), "\u{fffc}");
    assert_eq!(
        doc.resolved(doc.paragraph_count()),
        Err(FlatError::OutOfRange)
    );
    // Object resources.
    let with_object = (0..doc.paragraph_count())
        .find(|i| para_at(&doc, *i).is_some_and(|p| !p.anchored.is_empty()));
    if let Some(i) = with_object {
        let object = para_at(&doc, i).unwrap().anchored[0].clone();
        assert_eq!(doc.object_resource(object.id), Some(object.data.as_slice()));
    }
    // List counters: levels restart below a shallower item.
    let section_id = doc.section_ids()[0];
    let list_para = |doc: &mut RichDocument, level: u8| {
        let mut sub = doc.new_paragraph("x").unwrap();
        if let Some(Node::Paragraph(p)) = sub.nodes.get_mut(&sub.root) {
            p.props.list = Some(ListRef { list_id: 5, level });
        }
        let position = doc.block_ids(Some(section_id)).unwrap().len();
        apply(
            doc,
            vec![RichOp::InsertBlock {
                parent: Some(section_id),
                position,
                subtree: sub,
            }],
        );
    };
    let base = doc.paragraph_count();
    for level in [0, 1, 1, 0, 1] {
        list_para(&mut doc, level);
    }
    assert_eq!(doc.list_counters(0), Ok(None));
    let counters: Vec<_> = (base..base + 5)
        .map(|i| doc.list_counters(i).unwrap().unwrap())
        .collect();
    assert_eq!(counters[0].before[..2], [0, 0]);
    assert_eq!(counters[1].before[..2], [1, 0]);
    assert_eq!(counters[2].before[..2], [1, 1]);
    assert_eq!(counters[3].before[..2], [1, 2]);
    assert_eq!(counters[4].before[..2], [2, 0]);
    assert_eq!(counters[4].list_id, 5);
}

#[test]
fn the_index_follows_old_style_operations_and_clones() {
    let mut rng = Rng::new(11);
    let mut doc = random_body(&mut rng, 3, true);
    let before = doc.len();
    let first = doc.paragraph_ids()[0];
    let copy = doc.clone();
    apply(
        &mut doc,
        vec![atelier_text_model::rich::RichOp::InsertText {
            para: first,
            offset: 0,
            text: "zz".into(),
        }],
    );
    assert_eq!(doc.len(), before + 2);
    assert_eq!(copy.len(), before);
    compare(&doc);
    compare(&copy);
}

/// Conversions are consistent in every direction (property test, fixed seeds).
#[test]
fn conversions_are_consistent_in_every_direction() {
    for seed in 0..150 {
        let mut rng = Rng::new(7000 + seed);
        let count = 1 + rng.below(4);
        let doc = if seed % 2 == 0 {
            random_structured(&mut rng, count.min(3))
        } else {
            random_body(&mut rng, count, true)
        };
        let len = doc.len();
        // flat <-> (paragraph, offset)
        for p in 0..=len {
            let at = doc.locate(p).unwrap();
            assert_eq!(doc.position(at), Ok(p));
            assert_eq!(doc.locate(doc.position(at).unwrap()), Ok(at));
            assert!(at.offset <= doc.paragraph_flat(at.paragraph).unwrap().len());
        }
        // paragraph starts are the prefix sums of the lengths plus separators
        let mut sum = 0;
        for i in 0..doc.paragraph_count() {
            assert_eq!(doc.paragraph_start(i), Ok(sum));
            sum += doc.paragraph_flat(i).unwrap().len() + 1;
        }
        assert_eq!(sum - 1, len);
        let valid: Vec<usize> = (0..=len)
            .filter(|p| doc.check_position(*p).is_ok())
            .collect();
        for p in &valid {
            // snap_back is the identity on valid positions and goes backwards
            assert_eq!(doc.snap_back(*p), Ok(*p));
            if let Ok(Some(next)) = doc.next_grapheme(*p) {
                assert!(next > *p && doc.check_position(next).is_ok());
                assert_eq!(doc.prev_grapheme(next), Ok(Some(*p)));
            }
            // flat <-> (node id, offset in the text)
            let (node, offset) = doc.node_position(*p).unwrap();
            let before = doc.flat_position(node, offset, false).unwrap();
            let after = doc.flat_position(node, offset, true).unwrap();
            assert!(before <= *p && *p <= after, "{before} <= {p} <= {after}");
            assert_eq!(doc.node_position(before), Ok((node, offset)));
            assert_eq!(doc.node_position(after), Ok((node, offset)));
            if before == after {
                assert_eq!(before, *p);
            }
        }
        for p in 0..=len + 1 {
            let snapped = doc.snap_back(p.min(len)).unwrap();
            assert!(snapped <= p && doc.check_position(snapped).is_ok());
        }
        // The other direction, from every (node, offset) of every paragraph.
        for i in 0..doc.paragraph_count() {
            let Some(paragraph) = para_at(&doc, i) else {
                continue;
            };
            let start = doc.paragraph_start(i).unwrap();
            let node = doc.node_position(start).unwrap().0;
            for offset in 0..=paragraph.text.len() {
                for after in [false, true] {
                    match doc.flat_position(node, offset, after) {
                        Ok(flat) => {
                            assert_eq!(doc.node_position(flat), Ok((node, offset)));
                            assert!(
                                (start..=start + doc.paragraph_flat(i).unwrap().len())
                                    .contains(&flat)
                            );
                        }
                        Err(error) => {
                            assert_eq!(error, FlatError::NotBoundary);
                            assert!(!paragraph.text.is_char_boundary(offset));
                        }
                    }
                }
            }
            assert_eq!(
                doc.flat_position(node, paragraph.text.len() + 1, false),
                Err(FlatError::OutOfRange)
            );
        }
        assert_eq!(doc.node_position(len + 1), Err(FlatError::OutOfRange));
    }
}

#[test]
fn node_positions_follow_edits_and_structure() {
    use atelier_text_model::rich::flat::FlatEdit;
    let mut rng = Rng::new(31);
    let mut doc = random_structured(&mut rng, 3);
    let mut nodes = Vec::new();
    for i in 0..doc.paragraph_count() {
        let start = doc.paragraph_start(i).unwrap();
        nodes.push(doc.node_position(start).unwrap().0);
    }
    // Split and merge: the node of a surviving paragraph keeps its place.
    let target = (0..doc.paragraph_count())
        .find(|i| para_at(&doc, *i).is_some())
        .unwrap();
    let start = doc.paragraph_start(target).unwrap();
    doc.replace_range(start, start, "x\ny").unwrap();
    let after: Vec<_> = (0..doc.paragraph_count())
        .map(|i| {
            doc.node_position(doc.paragraph_start(i).unwrap())
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(after.len(), nodes.len() + 1);
    for (i, node) in after.iter().enumerate() {
        let start = doc.paragraph_start(i).unwrap();
        assert_eq!(doc.flat_position(*node, 0, false), Ok(start));
    }
    doc.undo().unwrap();
    for (i, node) in nodes.iter().enumerate() {
        let start = doc.paragraph_start(i).unwrap();
        assert_eq!(doc.flat_position(*node, 0, false), Ok(start));
    }
}
