// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmarks (US-291): name rules, insert / move / rename / delete with undo,
//! listing and sorting. Expected values computed by hand.

mod common;
mod nav_common;

use atelier_text_model::navigation::*;
use atelier_text_model::rich::*;
use atelier_text_model::search::Zone;
use common::*;
use nav_common::*;

fn names(list: &[BookmarkInfo]) -> Vec<&str> {
    list.iter().map(|b| b.name.as_str()).collect()
}

/// Applies an edit, checks validity, then undo gives back the identical
/// document and redo the same result. Returns the document after the edit.
fn apply_checked(doc: &mut RichDocument, edit: &BookmarkEdit) -> RichDocument {
    let before = doc.clone();
    let undo = doc.apply(&edit.transaction).unwrap();
    doc.validate().unwrap();
    let after = doc.clone();
    doc.apply(&undo).unwrap();
    doc.validate().unwrap();
    assert_eq!(*doc, before);
    doc.apply(&edit.transaction).unwrap();
    assert_eq!(*doc, after);
    after
}

fn user_insert(
    doc: &mut RichDocument,
    name: &str,
    p: NodeId,
    s: usize,
    e: usize,
) -> Result<BookmarkEdit, BookmarkError> {
    insert_bookmark(doc, name, pos(p, s), pos(p, e), NameMode::User)
}

#[test]
fn name_rules() {
    let user = |n: &str| validate_bookmark_name(n, NameMode::User);
    for ok in ["a", "Intro", "x_1", "chap2_fin", "Été", "é1", "Résumé_2"] {
        assert_eq!(user(ok), Ok(()), "{ok}");
    }
    assert_eq!(user(""), Err(NameError::Empty));
    assert_eq!(user("1abc"), Err(NameError::BadFirstChar('1')));
    assert_eq!(user("_abc"), Err(NameError::BadFirstChar('_')));
    assert_eq!(user(" a"), Err(NameError::BadFirstChar(' ')));
    assert_eq!(user("a b"), Err(NameError::BadChar { ch: ' ', index: 1 }));
    assert_eq!(user("a-b"), Err(NameError::BadChar { ch: '-', index: 1 }));
    assert_eq!(user("ab."), Err(NameError::BadChar { ch: '.', index: 2 }));
    assert_eq!(
        user("a\u{a0}b"),
        Err(NameError::BadChar {
            ch: '\u{a0}',
            index: 1
        })
    );
    // Length limit: 40 characters (not bytes).
    assert_eq!(user(&"a".repeat(40)), Ok(()));
    assert_eq!(user(&"a".repeat(41)), Err(NameError::TooLong { chars: 41 }));
    assert_eq!(user(&"é".repeat(40)), Ok(()));
    assert_eq!(user(&"é".repeat(41)), Err(NameError::TooLong { chars: 41 }));
    // Internal names.
    let internal = |n: &str| validate_bookmark_name(n, NameMode::Internal);
    assert_eq!(internal("_Toc123"), Ok(()));
    assert_eq!(internal("_GoBack"), Ok(()));
    assert_eq!(internal("Toc"), Err(NameError::NotInternal));
    assert_eq!(
        internal("_a b"),
        Err(NameError::BadChar { ch: ' ', index: 2 })
    );
    assert!(is_hidden_name("_GoBack") && !is_hidden_name("GoBack"));
    // Keys: case ignored, accents kept, canonical equivalence ignored.
    assert_eq!(name_key("ABC"), name_key("abc"));
    assert_eq!(name_key("Été"), name_key("été"));
    assert_ne!(name_key("e"), name_key("é"));
    assert_eq!(name_key("e\u{301}"), name_key("é"));
}

#[test]
fn insert_move_and_undo() {
    let (mut doc, p0) = with_text("Hello world");
    let section = first_section(&doc);
    let p1 = add_para(&mut doc, section, "Second");

    let edit = user_insert(&mut doc, "Intro", p0, 0, 5).unwrap();
    assert!(edit.links.is_empty());
    let after = apply_checked(&mut doc, &edit);
    doc = after;
    let info = find_bookmark(&doc, "INTRO").unwrap();
    assert_eq!(
        (info.name.as_str(), info.paragraph, info.start, info.end),
        ("Intro", p0, 0, 5)
    );
    assert_eq!(
        (info.zone, info.section, info.paragraph_index),
        (Zone::Body, 0, 0)
    );

    // Reversed ends are swapped; a point bookmark is allowed.
    let edit = user_insert(&mut doc, "Point", p0, 6, 6).unwrap();
    doc = apply_checked(&mut doc, &edit);
    let edit = user_insert(&mut doc, "Rev", p0, 11, 6).unwrap();
    doc = apply_checked(&mut doc, &edit);
    let info = find_bookmark(&doc, "rev").unwrap();
    assert_eq!((info.start, info.end), (6, 11));

    // Same name (other case) in another paragraph: moved, spelling and id kept.
    let id = find_bookmark(&doc, "Intro").unwrap().id;
    let edit = user_insert(&mut doc, "iNTRO", p1, 1, 3).unwrap();
    assert_eq!(edit.transaction.operations().len(), 2);
    doc = apply_checked(&mut doc, &edit);
    let info = find_bookmark(&doc, "Intro").unwrap();
    assert_eq!(
        (
            info.name.as_str(),
            info.id,
            info.paragraph,
            info.start,
            info.end
        ),
        ("Intro", id, p1, 1, 3)
    );
    assert_eq!(list_bookmarks(&doc, BookmarkOrder::Name, true).len(), 3);
    assert!(
        doc.paragraph(p0)
            .unwrap()
            .bookmarks
            .iter()
            .all(|b| b.name != "Intro")
    );

    // Same paragraph move: one operation.
    let edit = user_insert(&mut doc, "Intro", p1, 2, 6).unwrap();
    assert_eq!(edit.transaction.operations().len(), 1);
    doc = apply_checked(&mut doc, &edit);
    let info = find_bookmark(&doc, "Intro").unwrap();
    assert_eq!((info.start, info.end), (2, 6));
}

#[test]
fn insert_errors() {
    let (mut doc, p0) = with_text("e\u{301}x");
    let section = first_section(&doc);
    let p1 = add_para(&mut doc, section, "other");
    let before = doc.clone();
    assert_eq!(
        user_insert(&mut doc, "1x", p0, 0, 0).err(),
        Some(BookmarkError::Name(NameError::BadFirstChar('1')))
    );
    assert_eq!(
        insert_bookmark(&mut doc, "a", pos(p0, 0), pos(p1, 1), NameMode::User).err(),
        Some(BookmarkError::RangeSpansParagraphs)
    );
    assert_eq!(
        user_insert(&mut doc, "a", p0, 0, 99).err(),
        Some(BookmarkError::Model(RichError::OffsetOutOfBounds {
            offset: 99,
            len: 4
        }))
    );
    // Inside the grapheme "e" + U+0301.
    assert_eq!(
        user_insert(&mut doc, "a", p0, 1, 3).err(),
        Some(BookmarkError::Model(RichError::NotGraphemeBoundary {
            offset: 1
        }))
    );
    let section = first_section(&doc);
    assert!(matches!(
        user_insert(&mut doc, "a", section, 0, 0).err(),
        Some(BookmarkError::Model(RichError::WrongNodeKind { .. }))
    ));
    assert_eq!(doc, before);
}

#[test]
fn rename_updates_links_in_one_transaction() {
    let (mut doc, p0) = with_text("0123456789");
    let section = first_section(&doc);
    let p1 = add_para(&mut doc, section, "abcdef");
    bookmark_at(&mut doc, "Intro", p0, 1, 4);
    bookmark_at(&mut doc, "Other", p1, 0, 1);
    let l1 = add_link(&mut doc, p0, 5, 8, "INTRO");
    let l2 = add_link(&mut doc, p1, 2, 4, "Intro");
    let l3 = add_link(&mut doc, p1, 4, 6, "Other");
    let external = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p0,
            item: Inline::Link(Link {
                id: external,
                start: 8,
                end: 10,
                target: LinkTarget::External("https://example.org".into()),
                tooltip: None,
            }),
        },
    );
    let edit = rename_bookmark(&doc, "intro", "Début", NameMode::User).unwrap();
    assert_eq!(edit.links.len(), 2);
    assert_eq!(
        edit.links
            .iter()
            .map(|l| (l.paragraph, l.link))
            .collect::<Vec<_>>(),
        vec![(p0, l1), (p1, l2)]
    );
    let after = apply_checked(&mut doc, &edit);
    assert!(find_bookmark(&after, "Intro").is_none());
    let info = find_bookmark(&after, "début").unwrap();
    assert_eq!((info.name.as_str(), info.start, info.end), ("Début", 1, 4));
    let target = |doc: &RichDocument, p, id| {
        doc.paragraph(p)
            .unwrap()
            .links
            .iter()
            .find(|l| l.id == id)
            .unwrap()
            .target
            .clone()
    };
    assert_eq!(target(&after, p0, l1), LinkTarget::Bookmark("Début".into()));
    assert_eq!(target(&after, p1, l2), LinkTarget::Bookmark("Début".into()));
    assert_eq!(target(&after, p1, l3), LinkTarget::Bookmark("Other".into()));
    assert_eq!(
        target(&after, p0, external),
        LinkTarget::External("https://example.org".into())
    );
}

#[test]
fn rename_rules() {
    let (mut doc, p0) = with_text("0123456789");
    bookmark_at(&mut doc, "Intro", p0, 1, 4);
    bookmark_at(&mut doc, "Other", p0, 5, 6);
    assert_eq!(
        rename_bookmark(&doc, "Intro", "other", NameMode::User).err(),
        Some(BookmarkError::NameTaken("other".into()))
    );
    assert_eq!(
        rename_bookmark(&doc, "Nope", "x", NameMode::User).err(),
        Some(BookmarkError::NotFound("Nope".into()))
    );
    assert!(matches!(
        rename_bookmark(&doc, "Intro", "a b", NameMode::User).err(),
        Some(BookmarkError::Name(_))
    ));
    // Case-only change is a rename of the bookmark itself.
    let edit = rename_bookmark(&doc, "Intro", "INTRO", NameMode::User).unwrap();
    let after = apply_checked(&mut doc, &edit);
    assert_eq!(find_bookmark(&after, "intro").unwrap().name, "INTRO");
    // Same name: nothing to do.
    let edit = rename_bookmark(&doc, "intro", "INTRO", NameMode::User).unwrap();
    assert!(edit.transaction.operations().is_empty());
}

#[test]
fn delete_leaves_links_and_reports_them() {
    let (mut doc, p0) = with_text("0123456789");
    bookmark_at(&mut doc, "Intro", p0, 1, 4);
    let link = add_link(&mut doc, p0, 5, 8, "intro");
    let edit = delete_bookmark(&doc, "INTRO").unwrap();
    assert_eq!(edit.links.len(), 1);
    assert_eq!(edit.links[0].link, link);
    let after = apply_checked(&mut doc, &edit);
    assert!(find_bookmark(&after, "Intro").is_none());
    // The link is untouched, so it works again after undo.
    assert_eq!(
        after.paragraph(p0).unwrap().links[0].target,
        LinkTarget::Bookmark("intro".into())
    );
    assert_eq!(
        delete_bookmark(&doc, "zzz").err(),
        Some(BookmarkError::NotFound("zzz".into()))
    );
}

#[test]
fn hidden_bookmarks_are_kept_but_not_listed_by_default() {
    let (mut doc, p0) = with_text("0123456789");
    bookmark_at(&mut doc, "Visible", p0, 0, 1);
    bookmark_at(&mut doc, "_Toc1", p0, 2, 3);
    bookmark_at(&mut doc, "_GoBack", p0, 4, 4);
    assert_eq!(
        names(&list_bookmarks(&doc, BookmarkOrder::Name, false)),
        ["Visible"]
    );
    assert_eq!(
        names(&list_bookmarks(&doc, BookmarkOrder::Position, true)),
        ["Visible", "_Toc1", "_GoBack"]
    );
    assert!(find_bookmark(&doc, "_toc1").unwrap().hidden);
    // A user cannot create one...
    assert!(user_insert(&mut doc, "_mine", p0, 0, 0).is_err());
    // ...but can delete it by name.
    let edit = delete_bookmark(&doc, "_GoBack").unwrap();
    apply_checked(&mut doc, &edit);
}

#[test]
fn sort_by_name_ignores_case_and_accents() {
    let (mut doc, p0) = with_text("0123456789");
    for (i, n) in [
        "zèbre", "Zeta", "alpha", "Élan", "ecole", "Eclair", "éle", "ele",
    ]
    .iter()
    .enumerate()
    {
        bookmark_at(&mut doc, n, p0, i, i + 1);
    }
    // Keys: alpha, eclair, ecole, elan, ele (x2, tie: reading order, "éle"
    // first), zebre, zeta.
    assert_eq!(
        names(&list_bookmarks(&doc, BookmarkOrder::Name, false)),
        [
            "alpha", "Eclair", "ecole", "Élan", "éle", "ele", "zèbre", "Zeta"
        ]
    );
    assert_eq!(
        names(&list_bookmarks(&doc, BookmarkOrder::Position, false)),
        [
            "zèbre", "Zeta", "alpha", "Élan", "ecole", "Eclair", "éle", "ele"
        ]
    );
}

#[test]
fn sort_by_position_across_zones() {
    let (mut doc, p0) = with_text("0123456789");
    let section = first_section(&doc);
    let hdr = add_header(&mut doc, section, "header");
    let table = add_table_to(&mut doc, section, 1, 2);
    let cell = cells(&doc, table)[0][1];
    let cp = cell_para(&doc, cell);
    set_text(&mut doc, cp, "cell text");
    let last = add_para(&mut doc, section, "later");
    let footer = {
        let subtree = doc.new_header_footer("foot").unwrap();
        let hf = subtree.root;
        run_checked(
            &mut doc,
            RichOp::AttachHeaderFooter {
                section,
                kind: HfKind::Footer,
                variant: HfVariant::Default,
                subtree,
            },
        );
        doc.block_ids(Some(hf)).unwrap()[0]
    };
    bookmark_at(&mut doc, "tbl", cp, 2, 4);
    bookmark_at(&mut doc, "foot", footer, 0, 1);
    bookmark_at(&mut doc, "later", last, 0, 5);
    bookmark_at(&mut doc, "b1", p0, 3, 3);
    bookmark_at(&mut doc, "hdr", hdr, 1, 2);
    bookmark_at(&mut doc, "b0", p0, 1, 2);
    let list = list_bookmarks(&doc, BookmarkOrder::Position, false);
    // Reading order of the search module: header, body, footer. Body: p0,
    // then the two cell paragraphs of the 1x2 table, then "later".
    assert_eq!(names(&list), ["hdr", "b0", "b1", "tbl", "later", "foot"]);
    assert_eq!(
        list.iter()
            .map(|b| (b.zone, b.paragraph_index))
            .collect::<Vec<_>>(),
        vec![
            (Zone::Header, 0),
            (Zone::Body, 1),
            (Zone::Body, 1),
            (Zone::Table, 3),
            (Zone::Body, 4),
            (Zone::Footer, 5)
        ]
    );
    assert_eq!(
        names(&list_bookmarks(&doc, BookmarkOrder::Name, false)),
        ["b0", "b1", "foot", "hdr", "later", "tbl"]
    );
}

/// A document as an import can produce it: "Titre" and "titre" coexist.
fn imported_duplicates() -> (RichDocument, NodeId) {
    let (mut doc, p0) = with_text("0123456789");
    raw_bookmark(&mut doc, "Titre", p0, 0, 2);
    raw_bookmark(&mut doc, "titre", p0, 4, 6);
    (doc, p0)
}

#[test]
fn exact_name_wins_over_case_variants() {
    let (doc, p0) = imported_duplicates();
    let m = lookup_bookmark(&doc, "titre").unwrap();
    assert_eq!(
        (m.info.name.as_str(), m.info.start, m.ambiguous),
        ("titre", 4, false)
    );
    let m = lookup_bookmark(&doc, "Titre").unwrap();
    assert_eq!(
        (m.info.name.as_str(), m.info.start, m.ambiguous),
        ("Titre", 0, false)
    );
    // No exact match: the first in reading order, flagged.
    let m = lookup_bookmark(&doc, "TITRE").unwrap();
    assert_eq!((m.info.name.as_str(), m.ambiguous), ("Titre", true));
    assert_eq!(find_bookmark(&doc, "tItRe").unwrap().name, "Titre");

    // rename("titre", ...) renames "titre", not "Titre".
    let edit = rename_bookmark(&doc, "titre", "autre", NameMode::User).unwrap();
    assert!(!edit.ambiguous);
    let mut d = doc.clone();
    let after = apply_checked(&mut d, &edit);
    let mut names: Vec<_> = list_bookmarks(&after, BookmarkOrder::Position, true)
        .into_iter()
        .map(|b| b.name)
        .collect();
    names.sort();
    assert_eq!(names, ["Titre", "autre"]);
    // Ambiguous input is flagged on every command.
    assert!(
        rename_bookmark(&doc, "TITRE", "autre", NameMode::User)
            .unwrap()
            .ambiguous
    );
    assert!(delete_bookmark(&doc, "TITRE").unwrap().ambiguous);
    // delete("titre") removes the exact one.
    let mut d = doc.clone();
    let after = apply_checked(&mut d, &delete_bookmark(&doc, "titre").unwrap());
    assert_eq!(names_of(&after), ["Titre"]);
    // Moving by insertion also prefers the exact name.
    let mut d = doc.clone();
    let edit = user_insert(&mut d, "titre", p0, 8, 9).unwrap();
    assert!(!edit.ambiguous);
    let after = apply_checked(&mut d, &edit);
    assert_eq!(
        (
            find_bookmark(&after, "titre").unwrap().start,
            find_bookmark(&after, "Titre").unwrap().start
        ),
        (8, 0)
    );
}

fn names_of(doc: &RichDocument) -> Vec<String> {
    list_bookmarks(doc, BookmarkOrder::Position, true)
        .into_iter()
        .map(|b| b.name)
        .collect()
}

#[test]
fn rename_collision_is_detected_before_the_model() {
    let (doc, _) = imported_duplicates();
    for (old, new) in [("Titre", "titre"), ("titre", "Titre"), ("titre", "TITRE")] {
        assert_eq!(
            rename_bookmark(&doc, old, new, NameMode::User).err(),
            Some(BookmarkError::NameTaken(new.into())),
            "{old} -> {new}"
        );
    }
    // A clean document: another bookmark differing by case only is taken too...
    let (mut doc, p0) = with_text("0123456789");
    bookmark_at(&mut doc, "Alpha", p0, 0, 1);
    bookmark_at(&mut doc, "beta", p0, 2, 3);
    assert_eq!(
        rename_bookmark(&doc, "beta", "ALPHA", NameMode::User).err(),
        Some(BookmarkError::NameTaken("ALPHA".into()))
    );
    // ...but a bookmark may change the case of its own name.
    let edit = rename_bookmark(&doc, "beta", "BETA", NameMode::User).unwrap();
    apply_checked(&mut doc, &edit);
}

#[test]
fn composed_and_decomposed_names_are_the_same() {
    let decomposed = "e\u{301}te\u{301}";
    assert_eq!(validate_bookmark_name(decomposed, NameMode::User), Ok(()));
    assert_eq!(normalize_name(decomposed), "été");
    // 40 decomposed letters = 80 chars before NFC, 40 after.
    assert_eq!(
        validate_bookmark_name(&"e\u{301}".repeat(40), NameMode::User),
        Ok(())
    );
    assert_eq!(
        validate_bookmark_name(&"e\u{301}".repeat(41), NameMode::User),
        Err(NameError::TooLong { chars: 41 })
    );
    let (mut doc, p0) = with_text("0123456789");
    let edit = user_insert(&mut doc, decomposed, p0, 0, 1).unwrap();
    doc = apply_checked(&mut doc, &edit);
    // Stored composed; found by either spelling; the second spelling moves it.
    assert_eq!(names_of(&doc), ["été"]);
    assert!(find_bookmark(&doc, decomposed).is_some() && find_bookmark(&doc, "été").is_some());
    let edit = user_insert(&mut doc, "été", p0, 2, 3).unwrap();
    doc = apply_checked(&mut doc, &edit);
    assert_eq!(names_of(&doc), ["été"]);
    let edit = rename_bookmark(&doc, decomposed, "e\u{301}le\u{301}ve", NameMode::User).unwrap();
    let after = apply_checked(&mut doc, &edit);
    assert_eq!(names_of(&after), ["éléve"]);
}
