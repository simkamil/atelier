// SPDX-License-Identifier: AGPL-3.0-or-later
//! Independent acceptance checks for the retained DOCX editing prerequisite.
use atelier_core_wasm::rich_preview::preview_docx;
use atelier_core_wasm::rich_session::{RichSelection, RichSession, RichSessionError};
use atelier_fmt_ooxml::{Package, PartUri, TargetMode};

const REFERENCE: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn fixture(body: &str, styles: Option<&str>) -> Vec<u8> {
    let mut package = Package::new();
    let main = PartUri::parse("word/document.xml").unwrap();
    package
        .insert_part(
            main.clone(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#)
                .into_bytes(),
        )
        .unwrap();
    package
        .add_relationship(
            None,
            "doc",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "word/document.xml",
            TargetMode::Internal,
        )
        .unwrap();
    package
        .insert_part(
            PartUri::parse("custom/retained.bin").unwrap(),
            "application/octet-stream",
            vec![0, 1, 255, 37, 0],
        )
        .unwrap();
    if let Some(styles) = styles {
        package
            .insert_part(
                PartUri::parse("word/styles.xml").unwrap(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
                format!(r#"<w:styles xmlns:w="{W}">{styles}</w:styles>"#).into_bytes(),
            )
            .unwrap();
        package
            .add_relationship(
                Some(main),
                "styles",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
                "styles.xml",
                TargetMode::Internal,
            )
            .unwrap();
    }
    package.write().unwrap()
}

fn simple(text: &str) -> Vec<u8> {
    fixture(&format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"), None)
}

#[test]
fn retained_import_equals_preview_and_rejects_bad_input() {
    let mut session = RichSession::open(REFERENCE).unwrap();
    assert_eq!(session.revision(), 0);
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.preview().unwrap(), preview_docx(REFERENCE).unwrap());
    assert!(RichSession::open(b"not a docx").is_err());
    assert!(RichSession::open(&vec![0; 8 * 1024 * 1024 + 1]).is_err());
}

#[test]
fn replace_undo_redo_restore_render_model_metadata_and_selection() {
    let bytes = fixture(
        "<w:p><w:r><w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr><w:t>Bonjour</w:t></w:r></w:p><w:p><w:r><w:t>Intact</w:t></w:r></w:p>",
        None,
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let ids = session.imported().paragraph_ids().to_vec();
    let original = session
        .imported()
        .document
        .paragraph(ids[0])
        .unwrap()
        .clone();
    let other = session
        .imported()
        .document
        .paragraph(ids[1])
        .unwrap()
        .clone();
    let package = session.imported().package.write().unwrap();
    let before = session.preview().unwrap();
    let selection = RichSelection {
        paragraph: ids[0],
        anchor: 1,
        focus: 4,
    };
    session.select(0, selection).unwrap();
    assert_eq!(session.revision(), 0);
    let edited = session.replace(0, ids[0], 1, 4, "Été").unwrap();
    assert_eq!(edited.text, "BÉtéour\nIntact");
    assert_eq!(session.revision(), 1);
    assert_eq!(
        session.selection(),
        RichSelection {
            paragraph: ids[0],
            anchor: 6,
            focus: 6
        }
    );
    assert_eq!(
        session
            .imported()
            .document
            .paragraph(ids[0])
            .unwrap()
            .attrs_at(1)
            .bold,
        Some(true)
    );
    assert_eq!(
        session.imported().document.paragraph(ids[1]).unwrap(),
        &other
    );
    assert_eq!(session.imported().package.write().unwrap(), package);
    assert_eq!(session.undo(1).unwrap(), before);
    assert_eq!(session.selection(), selection);
    assert_eq!(
        session.imported().document.paragraph(ids[0]).unwrap(),
        &original
    );
    assert_eq!(session.redo(2).unwrap(), edited);
    assert_eq!(session.revision(), 3);
    assert_eq!(session.imported().package.write().unwrap(), package);
}

#[test]
fn rejected_edits_preserve_redo_selection_revision_and_preview() {
    let mut session = RichSession::open(&simple("Ae&#x301;B")).unwrap();
    let id = session.imported().paragraph_ids()[0];
    session.replace(0, id, 0, 0, "X").unwrap();
    let expected = session.undo(1).unwrap();
    let selection = session.selection();
    let unknown = session.imported().document.clone().alloc_id().unwrap();
    for (revision, para, start, end, text) in [
        (0, id, 0, 0, "X"),
        (2, id, 2, 2, "X"), // middle of e + combining acute grapheme
        (2, id, 3, 3, "X"), // middle of UTF-8 scalar
        (2, id, 6, 6, "X"), // past end
        (2, id, 4, 1, "X"),
        (2, unknown, 0, 0, "X"),
        (2, id, 0, 0, "\n"),
        (2, id, 0, 0, "\r"),
    ] {
        assert!(session.replace(revision, para, start, end, text).is_err());
        assert_eq!(session.revision(), 2);
        assert_eq!(session.selection(), selection);
        assert!(session.can_redo());
        assert!(!session.can_undo());
        assert_eq!(session.preview().unwrap(), expected);
    }
    assert!(matches!(
        session.undo(1),
        Err(RichSessionError::StaleRevision { .. })
    ));
    assert!(
        session
            .select(
                2,
                RichSelection {
                    paragraph: id,
                    anchor: 2,
                    focus: 2
                }
            )
            .is_err()
    );
    assert_eq!(session.revision(), 2);
    assert_eq!(session.selection(), selection);
}

#[test]
fn styles_follow_text_and_insertions_inherit_preceding_character() {
    let styles = r#"<w:style w:type="character" w:styleId="Red"><w:rPr><w:color w:val="FF0000"/></w:rPr></w:style><w:style w:type="character" w:styleId="Blue"><w:rPr><w:color w:val="0000FF"/></w:rPr></w:style>"#;
    let bytes = fixture(
        r#"<w:p><w:r><w:rPr><w:rStyle w:val="Red"/><w:b/></w:rPr><w:t>AB</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Blue"/><w:i/></w:rPr><w:t>CD</w:t></w:r></w:p>"#,
        Some(styles),
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let original_styles = session.imported().run_styles.clone();
    let initial = session.preview().unwrap();
    let rendered = session.replace(0, id, 2, 2, "x").unwrap();
    let page = atelier_render::DisplayList::decode(&rendered.pages[0]).unwrap();
    let colors: Vec<_> = page
        .commands
        .iter()
        .filter_map(|command| match command {
            atelier_render::Command::FillPath { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(colors.len(), 5, "A, B, inserted x, C and D are drawn");
    for color in &colors[..3] {
        assert_eq!(
            (color.red, color.green, color.blue),
            (1.0, 0.0, 0.0),
            "inserted x visibly inherits the red style"
        );
    }
    for color in &colors[3..] {
        assert_eq!(
            (color.red, color.green, color.blue),
            (0.0, 0.0, 1.0),
            "surviving CD retain blue"
        );
    }
    let runs = &session.imported().run_styles;
    assert!(runs.iter().any(|r| r.paragraph == id
        && r.start <= 2
        && r.end >= 3
        && r.style_id.as_deref() == Some("Red")));
    assert!(runs.iter().any(|r| r.paragraph == id
        && r.start == 3
        && r.end == 5
        && r.style_id.as_deref() == Some("Blue")));
    let para = session.imported().document.paragraph(id).unwrap();
    assert_eq!(para.attrs_at(2).bold, Some(true));
    assert_eq!(para.attrs_at(3).italic, Some(true));
    assert_eq!(session.undo(1).unwrap(), initial);
    assert_eq!(session.imported().run_styles, original_styles);
    assert_eq!(session.redo(2).unwrap(), rendered);
    session.replace(3, id, 0, 0, "z").unwrap();
    assert_eq!(
        session
            .imported()
            .document
            .paragraph(id)
            .unwrap()
            .attrs_at(0)
            .bold,
        Some(true)
    );
    assert!(
        session
            .imported()
            .run_styles
            .iter()
            .any(|r| r.start == 0 && r.end >= 1 && r.style_id.as_deref() == Some("Red"))
    );
}

#[test]
fn deleting_across_style_boundary_keeps_surviving_style_identities() {
    let bytes = fixture(
        r#"<w:p><w:r><w:rPr><w:rStyle w:val="One"/></w:rPr><w:t>AB</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Two"/></w:rPr><w:t>CD</w:t></w:r></w:p>"#,
        Some(
            r#"<w:style w:type="character" w:styleId="One"/><w:style w:type="character" w:styleId="Two"/>"#,
        ),
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let before = session.imported().run_styles.clone();
    assert_eq!(session.replace(0, id, 1, 3, "").unwrap().text, "AD");
    let runs = &session.imported().run_styles;
    assert!(
        runs.iter()
            .any(|r| r.start == 0 && r.end == 1 && r.style_id.as_deref() == Some("One"))
    );
    assert!(
        runs.iter()
            .any(|r| r.start == 1 && r.end == 2 && r.style_id.as_deref() == Some("Two"))
    );
    session.undo(1).unwrap();
    assert_eq!(session.imported().run_styles, before);
}

#[test]
fn source_breaks_and_symbols_shift_delete_and_restore_exactly() {
    let bytes = fixture(
        r#"<w:p><w:r><w:t>A</w:t><w:br w:type="page"/><w:t>B</w:t><w:sym w:font="Symbol" w:char="0041"/><w:t>C</w:t></w:r></w:p>"#,
        None,
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let breaks = session.imported().breaks.clone();
    let symbols = session.imported().symbols.clone();
    assert_eq!(breaks.len(), 1);
    assert_eq!(symbols.len(), 1);
    let expected = session.preview().unwrap();
    session.replace(0, id, 0, 0, "é").unwrap();
    assert_eq!(session.imported().breaks[0].offset, breaks[0].offset + 2);
    assert_eq!(session.imported().symbols[0].offset, symbols[0].offset + 2);
    let length = session
        .imported()
        .document
        .paragraph(id)
        .unwrap()
        .text
        .len();
    session.replace(1, id, 0, length, "").unwrap();
    assert!(session.imported().breaks.is_empty());
    assert!(session.imported().symbols.is_empty());
    session.undo(2).unwrap();
    assert_eq!(session.undo(3).unwrap(), expected);
    assert_eq!(session.imported().breaks, breaks);
    assert_eq!(session.imported().symbols, symbols);
}

#[test]
fn noops_preserve_redo_and_new_edit_clears_it() {
    let mut session = RichSession::open(&simple("AB")).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let before = session.preview().unwrap();
    assert_eq!(session.undo(0).unwrap(), before);
    assert_eq!(session.redo(0).unwrap(), before);
    assert_eq!(session.revision(), 0);
    session.replace(0, id, 0, 0, "X").unwrap();
    session.undo(1).unwrap();
    session.replace(2, id, 0, 0, "").unwrap();
    assert_eq!(session.revision(), 2);
    assert!(session.can_redo());
    session.replace(2, id, 0, 0, "Y").unwrap();
    assert!(!session.can_redo());
    assert_eq!(session.revision(), 3);
}

#[test]
fn oversized_history_entry_is_rejected_before_mutation() {
    let mut session = RichSession::open_with_history_budget(&simple("AB"), 0).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let before = session.preview().unwrap();
    let selection = session.selection();
    assert!(session.replace(0, id, 0, 0, "C").is_err());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.selection(), selection);
    assert!(!session.can_undo());
    assert_eq!(session.preview().unwrap(), before);
}

#[test]
fn history_budget_counts_owned_formatting_strings() {
    let font = "F".repeat(4096);
    let bytes = fixture(
        &format!(
            r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="{font}"/></w:rPr><w:t>AB</w:t></w:r></w:p>"#
        ),
        None,
    );
    let mut session = RichSession::open_with_history_budget(&bytes, 4096).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let before = session.preview().unwrap();
    assert!(
        session.replace(0, id, 1, 1, "C").is_err(),
        "retained font strings exceed the budget even when text is short"
    );
    assert_eq!(session.revision(), 0);
    assert!(!session.can_undo());
    assert_eq!(session.preview().unwrap(), before);
}

#[test]
fn undo_restores_actual_selection_when_replacing_another_range() {
    let mut session = RichSession::open(&simple("ABCD")).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let selection = RichSelection {
        paragraph: id,
        anchor: 4,
        focus: 4,
    };
    session.select(0, selection).unwrap();
    session.replace(0, id, 0, 1, "X").unwrap();
    session.undo(1).unwrap();
    assert_eq!(session.selection(), selection);
}

#[test]
fn layout_failure_after_candidate_edit_restores_model_and_redo() {
    let bytes = fixture(
        r#"<w:p><w:r><w:rPr><w:sz w:val="800"/></w:rPr><w:t>A</w:t></w:r></w:p>"#,
        None,
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let id = session.imported().paragraph_ids()[0];
    session.replace(0, id, 0, 1, "B").unwrap();
    let before = session.undo(1).unwrap();
    let model = session.imported().document.clone();
    let styles = session.imported().run_styles.clone();
    let selection = session.selection();
    // Well below the byte/history ceilings, but too many pages at 400 pt.
    assert!(session.replace(2, id, 0, 0, &"A".repeat(1000)).is_err());
    assert_eq!(session.imported().document, model);
    assert_eq!(session.imported().run_styles, styles);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.revision(), 2);
    assert!(session.can_redo());
    assert!(!session.can_undo());
    assert_eq!(session.preview().unwrap(), before);
    assert_eq!(session.redo(2).unwrap().text, "B");
}

#[test]
fn failed_large_render_preserves_existing_history_and_model() {
    let mut session = RichSession::open(&simple("AB")).unwrap();
    let id = session.imported().paragraph_ids()[0];
    session.replace(0, id, 0, 0, "X").unwrap();
    let before = session.undo(1).unwrap();
    let selection = session.selection();
    let large = "A".repeat(128 * 1024 + 1);
    assert!(session.replace(2, id, 0, 0, &large).is_err());
    assert_eq!(session.revision(), 2);
    assert_eq!(session.selection(), selection);
    assert!(session.can_redo());
    assert!(!session.can_undo());
    assert_eq!(session.preview().unwrap(), before);
}

#[test]
fn projected_text_limit_counts_all_paragraphs_and_separators() {
    let bytes = fixture(
        "<w:p><w:r><w:t>AB</w:t></w:r></w:p><w:p><w:r><w:t>CD</w:t></w:r></w:p>",
        None,
    );
    let mut session = RichSession::open(&bytes).unwrap();
    let id = session.imported().paragraph_ids()[0];
    let before = session.preview().unwrap();
    let replacement = "A".repeat(128 * 1024 - 4);
    assert!(matches!(
        session.replace(0, id, 0, 0, &replacement),
        Err(RichSessionError::Limit)
    ));
    assert_eq!(session.revision(), 0);
    assert!(!session.can_undo());
    assert_eq!(session.preview().unwrap(), before);
}

#[test]
fn empty_import_and_tab_insertion_are_supported() {
    let mut session = RichSession::open(&fixture("", None)).unwrap();
    let id = session.imported().paragraph_ids()[0];
    assert_eq!(session.preview().unwrap().text, "");
    assert_eq!(session.replace(0, id, 0, 0, "A\tB").unwrap().text, "A\tB");
    let before = session.preview().unwrap();
    for control in ["\0", "\u{000b}", "\u{2028}", "\u{2029}", "\u{feff}"] {
        assert!(matches!(
            session.replace(1, id, 0, 0, control),
            Err(RichSessionError::InvalidText)
        ));
        assert_eq!(session.preview().unwrap(), before);
        assert_eq!(session.revision(), 1);
    }
    assert_eq!(session.undo(1).unwrap().text, "");
}
