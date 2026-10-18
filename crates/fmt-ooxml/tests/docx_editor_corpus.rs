// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-218: the save path of the rich editor over the writer corpus. Each
//! document is imported, handed back as the editor does (a working copy of
//! the model in `ImportedDocx::document`) and written with and without
//! `regenerate_all`: no write notice may appear. The corpus is also exported
//! to `target/us218-corpus/` so that `tests/rich-editor-save.mjs` replays it
//! through the real WASM engine.
mod support;

use atelier_fmt_ooxml::docx_image::write_docx_with_pictures;
use atelier_fmt_ooxml::{DocxWriteOptions, Package, import_docx};
use std::path::PathBuf;

fn export_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/us218-corpus");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_editor_save_path_adds_no_notice_over_the_corpus() {
    let dir = export_dir();
    let documents: Vec<(String, Vec<u8>)> = support::corpus()
        .into_iter()
        .map(|(name, bytes)| (name.replace('/', "-"), bytes))
        .chain(
            support::built_documents()
                .into_iter()
                .map(|(name, bytes)| (format!("built-{name}.docx"), bytes)),
        )
        .collect();
    assert!(documents.len() >= 6);
    for (name, bytes) in &documents {
        std::fs::write(dir.join(name), bytes).unwrap();
        let mut imported = import_docx(bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let text = |i: &atelier_fmt_ooxml::ImportedDocx| {
            i.document
                .paragraph_ids()
                .into_iter()
                .map(|id| i.document.paragraph(id).unwrap().text.clone())
                .collect::<Vec<_>>()
        };
        let before = text(&imported);
        for regenerate_all in [false, true] {
            // The session's own document stays with the session; the writer
            // gets a copy.
            imported.document = imported.document.clone();
            let options = DocxWriteOptions {
                regenerate_all,
                ..DocxWriteOptions::default()
            };
            let written = write_docx_with_pictures(&mut imported, &options)
                .unwrap_or_else(|e| panic!("{name} regenerate={regenerate_all}: {e:?}"));
            assert!(
                written.notices.is_empty(),
                "{name} regenerate={regenerate_all}: {:?}",
                written.notices
            );
            let again = import_docx(&written.bytes).unwrap();
            assert_eq!(text(&again), before, "{name} regenerate={regenerate_all}");
            assert!(Package::read(&written.bytes).is_ok());
        }
    }
}
