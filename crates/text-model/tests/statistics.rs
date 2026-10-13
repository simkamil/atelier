// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hand-computed cases of the statistics rules (US-343). Every expected figure
//! was worked out by hand from the rules of `docs/us-343-statistics.md`.

mod common;

use atelier_model::selection::{Affinity, Selection, SelectionRange, TextPosition};
use atelier_text_model::rich::{
    Field, HfKind, HfVariant, Inline, NodeId, RichDocument, RichOp, Subtree,
};
use atelier_text_model::statistics::{
    CharUnit, CountingRules, Counts, StatisticsCache, StatisticsError, StatisticsOptions,
    count_paragraph, document, plain_paragraphs, plain_selection, plain_text, plain_text_range,
    selection,
};
use atelier_text_model::{PlainTextDocument, PlainTextLimits};
use common::*;

fn c(text: &str) -> (usize, usize, usize, usize) {
    let k = count_paragraph(text, &CountingRules::default());
    (
        k.words,
        k.characters,
        k.characters_with_spaces,
        k.paragraphs,
    )
}

fn unit(text: &str, char_unit: CharUnit) -> (usize, usize) {
    let rules = CountingRules {
        char_unit,
        ..CountingRules::default()
    };
    let k = count_paragraph(text, &rules);
    (k.characters, k.characters_with_spaces)
}

#[test]
fn empty_and_blank_text() {
    assert_eq!(c(""), (0, 0, 0, 0));
    assert_eq!(c("   "), (0, 0, 3, 1));
    assert_eq!(c("\t \u{b}"), (0, 0, 3, 1));
    let rules = CountingRules {
        count_empty_paragraphs: true,
        ..CountingRules::default()
    };
    assert_eq!(count_paragraph("", &rules).paragraphs, 1);
}

#[test]
fn spaces_separate_words_however_many() {
    assert_eq!(c("Hello world"), (2, 10, 11, 1));
    assert_eq!(c("  Hello   world  "), (2, 10, 17, 1));
}

#[test]
fn tabs_and_line_breaks_separate_and_count_as_spaces() {
    assert_eq!(c("a\tb"), (2, 2, 3, 1));
    assert_eq!(c("a\u{b}b"), (2, 2, 3, 1));
    assert_eq!(c("a\nb"), (2, 2, 3, 1));
    assert_eq!(c("a\u{2028}b"), (2, 2, 3, 1));
}

#[test]
fn no_break_spaces_do_not_separate_words() {
    // "10 km" with U+00A0: one word; the NBSP is a space for "no spaces".
    assert_eq!(c("10\u{a0}km"), (1, 4, 5, 1));
    assert_eq!(c("10\u{202f}km"), (1, 4, 5, 1));
    // A token made only of NBSP is not a word.
    assert_eq!(c("a \u{a0} b"), (2, 2, 5, 1));
}

#[test]
fn punctuation_sticks_to_the_word() {
    assert_eq!(c("Bonjour, monde!"), (2, 14, 15, 1));
    assert_eq!(c("(a) [b] «c»"), (3, 9, 11, 1));
    assert_eq!(c("word\u{2014}word"), (1, 9, 9, 1));
    assert_eq!(c("and/or"), (1, 6, 6, 1));
}

#[test]
fn isolated_dash_is_a_word() {
    assert_eq!(c("a - b"), (3, 3, 5, 1));
    assert_eq!(c("a \u{2014} b"), (3, 3, 5, 1));
    assert_eq!(c("-"), (1, 1, 1, 1));
}

#[test]
fn apostrophes_do_not_split() {
    assert_eq!(c("l'école"), (1, 7, 7, 1));
    assert_eq!(c("l\u{2019}école"), (1, 7, 7, 1));
    assert_eq!(c("aujourd'hui j\u{2019}arrive"), (2, 19, 20, 1));
}

#[test]
fn numbers_with_separators_are_single_words() {
    assert_eq!(c("1,234.56"), (1, 8, 8, 1));
    assert_eq!(c("3 000 000"), (3, 7, 9, 1));
    assert_eq!(c("3\u{a0}000"), (1, 4, 5, 1));
}

#[test]
fn characters_are_text_characters_not_bytes() {
    assert_eq!(c("é"), (1, 1, 1, 1));
    assert_eq!(c("日本"), (2, 2, 2, 1));
    assert_eq!(c("€"), (1, 1, 1, 1));
}

#[test]
fn character_units() {
    // e + combining acute
    assert_eq!(unit("e\u{301}", CharUnit::Scalar), (2, 2));
    assert_eq!(unit("e\u{301}", CharUnit::Grapheme), (1, 1));
    assert_eq!(unit("e\u{301}", CharUnit::Utf16), (2, 2));
    // one emoji outside the BMP
    assert_eq!(unit("\u{1f600}", CharUnit::Scalar), (1, 1));
    assert_eq!(unit("\u{1f600}", CharUnit::Utf16), (2, 2));
    // man-woman-girl ZWJ family: 5 scalars, 1 grapheme, 8 UTF-16 units
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    assert_eq!(unit(family, CharUnit::Scalar), (5, 5));
    assert_eq!(unit(family, CharUnit::Grapheme), (1, 1));
    assert_eq!(unit(family, CharUnit::Utf16), (8, 8));
    // flag, CRLF-free text with a space
    assert_eq!(unit("\u{1f1eb}\u{1f1f7} a", CharUnit::Grapheme), (2, 3));
    // combining accent and emoji are words
    assert_eq!(c("e\u{301}"), (1, 2, 2, 1));
    assert_eq!(c("\u{1f600}."), (1, 2, 2, 1));
    assert_eq!(c("\u{1f600} \u{1f600}"), (2, 2, 3, 1));
}

#[test]
fn ideographic_text_counts_one_word_per_character() {
    assert_eq!(c("日本語"), (3, 3, 3, 1));
    assert_eq!(c("日本語 text"), (4, 7, 8, 1));
    assert_eq!(c("abc日本def"), (4, 8, 8, 1));
    // kana count one each, ideographic punctuation is no word
    assert_eq!(c("こんにちは、世界。"), (7, 9, 9, 1));
    assert_eq!(c("。"), (0, 1, 1, 1));
    // Hangul is written with spaces: one word per token
    assert_eq!(c("안녕 하세요"), (2, 5, 6, 1));
    // a variation selector stays glued to its ideograph
    assert_eq!(c("\u{8fba}\u{e0100}"), (1, 2, 2, 1));
}

#[test]
fn invisible_characters_do_not_separate() {
    assert_eq!(c("a\u{200b}b"), (1, 3, 3, 1));
    assert_eq!(c("co\u{ad}operate"), (1, 10, 10, 1));
}

#[test]
fn counts_add_subtract_and_replace() {
    let a = count_paragraph("un deux", &CountingRules::default());
    let b = count_paragraph("trois", &CountingRules::default());
    assert_eq!(
        a + b,
        Counts {
            words: 3,
            characters: 11,
            characters_with_spaces: 12,
            paragraphs: 2
        }
    );
    assert_eq!((a + b) - b, a);
    assert_eq!(a.checked_sub(a + b), None);
    let mut total = a + b;
    total.replace(b, a);
    assert_eq!(total, a + a);
}

// ----- rich documents ---------------------------------------------------

fn text_at(doc: &mut RichDocument, para: NodeId, text: &str) {
    run_checked(
        doc,
        RichOp::InsertText {
            para,
            offset: 0,
            text: text.to_owned(),
        },
    );
}

fn push_para(doc: &mut RichDocument, parent: NodeId, text: &str) -> NodeId {
    let subtree = doc.new_paragraph(text).unwrap();
    let id = subtree.root;
    let position = doc.block_ids(Some(parent)).unwrap().len();
    run_checked(
        doc,
        RichOp::InsertBlock {
            parent: Some(parent),
            position,
            subtree,
        },
    );
    id
}

/// Body paragraphs "Un deux trois", "", "quatre<NBSP>cinq  six".
fn sample() -> (RichDocument, [NodeId; 3]) {
    let (mut doc, p0) = with_text("Un deux trois");
    let section = first_section(&doc);
    let p1 = push_para(&mut doc, section, "");
    let p2 = push_para(&mut doc, section, "quatre\u{a0}cinq  six");
    (doc, [p0, p1, p2])
}

fn pos(node: NodeId, offset: usize) -> TextPosition {
    TextPosition::new(node, offset, Affinity::After)
}

fn sel(ranges: &[(NodeId, usize, NodeId, usize)]) -> Selection {
    Selection::new(
        ranges
            .iter()
            .map(|&(a, ao, h, ho)| SelectionRange::new(pos(a, ao), pos(h, ho)))
            .collect(),
        0,
    )
    .unwrap()
}

fn tuple(s: atelier_text_model::statistics::Statistics) -> (usize, usize, usize, usize) {
    (
        s.words,
        s.characters,
        s.characters_with_spaces,
        s.paragraphs,
    )
}

#[test]
fn body_totals_skip_empty_paragraphs() {
    let (doc, _) = sample();
    let s = document(&doc, &StatisticsOptions::default());
    assert_eq!(tuple(s), (5, 24, 29, 2));
    assert_eq!((s.lines, s.pages), (None, None));
}

#[test]
fn fresh_document_is_all_zeros() {
    let doc = RichDocument::new();
    assert_eq!(
        tuple(document(&doc, &StatisticsOptions::default())),
        (0, 0, 0, 0)
    );
}

#[test]
fn layout_figures_are_restituted() {
    let (doc, [p0, ..]) = sample();
    let options = StatisticsOptions {
        lines: Some(42),
        pages: Some(3),
        ..StatisticsOptions::default()
    };
    let s = document(&doc, &options);
    assert_eq!((s.lines, s.pages), (Some(42), Some(3)));
    let s = selection(&doc, &sel(&[(p0, 0, p0, 2)]), &options).unwrap();
    assert_eq!((s.lines, s.pages), (Some(42), Some(3)));
    assert_eq!(tuple(s), (1, 2, 2, 1));
}

fn with_table(doc: &mut RichDocument) -> Vec<Vec<NodeId>> {
    let table = add_table(doc, 2, 2);
    let grid = cells(doc, table);
    let mut paras = Vec::new();
    for (row, texts) in grid.iter().zip([["a b", "c"], ["", "d e f"]]) {
        for (cell, text) in row.iter().zip(texts) {
            let para = doc.block_ids(Some(*cell)).unwrap()[0];
            if !text.is_empty() {
                text_at(doc, para, text);
            }
            paras.push(para);
        }
    }
    vec![paras]
}

#[test]
fn table_cells_are_counted() {
    let (mut doc, _) = sample();
    with_table(&mut doc);
    // (5, 24, 29, 2) + (6, 6, 9, 3)
    assert_eq!(
        tuple(document(&doc, &StatisticsOptions::default())),
        (11, 30, 38, 5)
    );
}

fn add_header(doc: &mut RichDocument, kind: HfKind, text: &str) {
    let subtree: Subtree = doc.new_header_footer(text).unwrap();
    let section = first_section(doc);
    run_checked(
        doc,
        RichOp::AttachHeaderFooter {
            section,
            kind,
            variant: HfVariant::Default,
            subtree,
        },
    );
}

#[test]
fn headers_and_footers_are_excluded_by_default() {
    let (mut doc, _) = sample();
    add_header(&mut doc, HfKind::Header, "En tête ici");
    add_header(&mut doc, HfKind::Footer, "Pied");
    assert_eq!(
        tuple(document(&doc, &StatisticsOptions::default())),
        (5, 24, 29, 2)
    );
    let options = StatisticsOptions {
        include_headers_footers: true,
        ..StatisticsOptions::default()
    };
    // header: 3 words, 9 chars, 11 with spaces; footer: 1, 4, 4
    assert_eq!(tuple(document(&doc, &options)), (9, 37, 44, 4));
}

#[test]
fn notes_and_text_boxes_option_changes_nothing_yet() {
    let (doc, _) = sample();
    let options = StatisticsOptions {
        include_notes_and_text_boxes: true,
        ..StatisticsOptions::default()
    };
    assert_eq!(
        document(&doc, &options),
        document(&doc, &StatisticsOptions::default())
    );
}

#[test]
fn field_result_counts_field_code_does_not() {
    let (mut doc, p) = with_text("Page 12 sur 40");
    let id = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Field(Field {
                id,
                start: 5,
                end: 7,
                instruction: " PAGE \\* MERGEFORMAT ".to_owned(),
            }),
        },
    );
    assert_eq!(
        tuple(document(&doc, &StatisticsOptions::default())),
        (4, 11, 14, 1)
    );
}

#[test]
fn selection_rules() {
    let (doc, [p0, p1, p2]) = sample();
    let o = StatisticsOptions::default();
    // caret
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p0, 4, p0, 4)]), &o).unwrap()),
        (0, 0, 0, 0)
    );
    // "ux t": both words are partial and count
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p0, 5, p0, 9)]), &o).unwrap()),
        (2, 3, 4, 1)
    );
    // reversed direction gives the same result
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p0, 9, p0, 5)]), &o).unwrap()),
        (2, 3, 4, 1)
    );
    // "ois" + empty paragraph + "qua"
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p0, 10, p2, 3)]), &o).unwrap()),
        (2, 6, 6, 2)
    );
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p2, 3, p0, 10)]), &o).unwrap()),
        (2, 6, 6, 2)
    );
    // an empty paragraph alone
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p1, 0, p1, 0)]), &o).unwrap()),
        (0, 0, 0, 0)
    );
    // whole document
    let all = selection(
        &doc,
        &sel(&[(p0, 0, p2, "quatre\u{a0}cinq  six".len())]),
        &o,
    )
    .unwrap();
    assert_eq!(tuple(all), (5, 24, 29, 2));
    // end of a paragraph to the start of the next: only the paragraph mark
    assert_eq!(
        tuple(selection(&doc, &sel(&[(p0, 13, p1, 0)]), &o).unwrap()),
        (0, 0, 0, 0)
    );
}

#[test]
fn selection_through_table_cells() {
    let (mut doc, _) = sample();
    let paras = with_table(&mut doc).remove(0);
    let o = StatisticsOptions::default();
    // " b" | "c" | "" | "d e"
    let s = selection(&doc, &sel(&[(paras[0], 1, paras[3], 3)]), &o).unwrap();
    assert_eq!(tuple(s), (4, 4, 6, 3));
}

#[test]
fn selection_ranges_are_merged_and_summed() {
    let (doc, [p0, _, p2]) = sample();
    let o = StatisticsOptions::default();
    // overlapping ranges count the overlap once: the whole first paragraph
    let s = selection(&doc, &sel(&[(p0, 0, p0, 7), (p0, 3, p0, 13)]), &o).unwrap();
    assert_eq!(tuple(s), (3, 11, 13, 1));
    // two separate ranges add up
    let s = selection(&doc, &sel(&[(p0, 0, p0, 2), (p2, 0, p2, 6)]), &o).unwrap();
    assert_eq!(tuple(s), (2, 8, 8, 2));
    // two disjoint ranges of the same paragraph stay one paragraph
    let s = selection(&doc, &sel(&[(p0, 0, p0, 2), (p0, 8, p0, 13)]), &o).unwrap();
    assert_eq!(tuple(s), (2, 7, 7, 1));
}

#[test]
fn cut_word_counts_in_both_halves() {
    let (doc, [p0, ..]) = sample();
    let o = StatisticsOptions::default();
    let left = selection(&doc, &sel(&[(p0, 0, p0, 5)]), &o).unwrap(); // "Un de"
    let right = selection(&doc, &sel(&[(p0, 5, p0, 13)]), &o).unwrap(); // "ux trois"
    let union = selection(&doc, &sel(&[(p0, 0, p0, 13)]), &o).unwrap();
    assert_eq!((left.words, right.words, union.words), (2, 2, 3));
    assert_eq!(left.characters + right.characters, union.characters);
    assert_eq!(
        left.characters_with_spaces + right.characters_with_spaces,
        union.characters_with_spaces
    );
}

#[test]
fn selection_in_a_header_and_errors() {
    let (mut doc, [p0, ..]) = sample();
    add_header(&mut doc, HfKind::Header, "En tête ici");
    let section = first_section(&doc);
    let header = match doc.node(section) {
        Some(atelier_text_model::rich::Node::Section(s)) => s.headers[0].1,
        _ => unreachable!(),
    };
    let hp = doc.block_ids(Some(header)).unwrap()[0];
    let o = StatisticsOptions::default();
    assert_eq!(
        tuple(selection(&doc, &sel(&[(hp, 0, hp, 6)]), &o).unwrap()),
        (2, 4, 5, 1)
    );
    assert_eq!(
        selection(&doc, &sel(&[(p0, 0, hp, 2)]), &o),
        Err(StatisticsError::RangeAcrossStories)
    );
    assert_eq!(
        selection(&doc, &sel(&[(p0, 0, p0, 99)]), &o),
        Err(StatisticsError::OffsetOutOfBounds {
            offset: 99,
            len: 13
        })
    );
    assert_eq!(
        selection(&doc, &sel(&[(section, 0, section, 0)]), &o),
        Err(StatisticsError::NotAParagraph(section))
    );
}

#[test]
fn selection_offset_inside_a_character_is_rejected() {
    let (doc, [_, _, p2]) = sample();
    // "quatre" + NBSP (2 bytes at 6..8)
    let r = selection(&doc, &sel(&[(p2, 0, p2, 7)]), &StatisticsOptions::default());
    assert_eq!(r, Err(StatisticsError::NotCharBoundary { offset: 7 }));
}

#[test]
fn cache_follows_edits() {
    let (mut doc, [p0, p1, _]) = sample();
    let o = StatisticsOptions::default();
    let mut cache = StatisticsCache::build(&doc, o);
    assert_eq!(cache.statistics(), document(&doc, &o));
    text_at(&mut doc, p1, "nouveau paragraphe");
    cache.update_paragraph(&doc, p1).unwrap();
    assert_eq!(cache.statistics(), document(&doc, &o));
    assert_eq!(tuple(cache.statistics()), (7, 41, 47, 3));
    // a new paragraph needs sync
    let section = first_section(&doc);
    let p3 = push_para(&mut doc, section, "fin");
    assert_eq!(
        cache.update_paragraph(&doc, p3),
        Err(StatisticsError::NotCached(p3))
    );
    cache.sync(&doc, &[]);
    assert_eq!(cache.statistics(), document(&doc, &o));
    // removal
    run_checked(
        &mut doc,
        RichOp::RemoveBlock {
            parent: Some(section),
            position: 0,
        },
    );
    cache.sync(&doc, &[]);
    assert_eq!(cache.statistics(), document(&doc, &o));
    assert_eq!(cache.paragraph_counts(p0), None);
    cache.set_layout(Some(5), Some(1));
    assert_eq!(
        (cache.statistics().lines, cache.statistics().pages),
        (Some(5), Some(1))
    );
}

// ----- plain text -------------------------------------------------------

fn plain(text: &str) -> PlainTextDocument {
    PlainTextDocument::from_utf8(text.as_bytes(), PlainTextLimits::default()).unwrap()
}

#[test]
fn plain_text_paragraphs_and_totals() {
    let o = StatisticsOptions::default();
    let text = "Un deux\r\n\r\ntrois\n";
    assert_eq!(plain_paragraphs(text).len(), 4);
    assert_eq!(tuple(plain_text(text, &o)), (3, 11, 12, 2));
    assert_eq!(tuple(plain_text("", &o)), (0, 0, 0, 0));
    assert_eq!(tuple(plain_text("a\rb\u{2029}c", &o)), (3, 3, 3, 3));
    // same algorithm as the rich model
    let (doc, _) = sample();
    assert_eq!(
        tuple(plain_text("Un deux trois\n\nquatre\u{a0}cinq  six", &o)),
        tuple(document(&doc, &o))
    );
}

#[test]
fn plain_selection_and_ranges() {
    let o = StatisticsOptions::default();
    let mut d = plain("Un deux trois\n\nquatre cinq");
    assert_eq!(tuple(plain_selection(&d, &o)), (0, 0, 0, 0));
    d.set_selection(atelier_text_model::Selection {
        anchor: 20,
        focus: 5,
    })
    .unwrap();
    // "ux trois\n\nquat": 2 words + 1 word; pieces "ux trois", "", "quatr"
    assert_eq!(tuple(plain_selection(&d, &o)), (3, 12, 13, 2));
    // a range ending right after a line break has no extra paragraph
    let t = "ab\ncd";
    assert_eq!(tuple(plain_text_range(t, 0..3, &o).unwrap()), (1, 2, 2, 1));
    assert_eq!(
        plain_text_range(t, 0..9, &o),
        Err(StatisticsError::OffsetOutOfBounds { offset: 9, len: 5 })
    );
    assert_eq!(
        plain_text_range("é", 0..1, &o),
        Err(StatisticsError::NotCharBoundary { offset: 1 })
    );
}
