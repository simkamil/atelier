// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-307: writes sample DOCX files for the LibreOffice check
//! (`tools/check-docx-writer-libreoffice.py`) and for the manual comparison
//! in Word (`docs/us-307-docx-writer.md`).
//!
//! `cargo run -p atelier-fmt-ooxml --example write_docx_samples -- <dir>`
//! writes `<dir>/roundtrip`, `<dir>/new`, `<dir>/modified`, `<dir>/word` and a
//! tab-separated `<dir>/manifest.tsv` (`kind`, `file`, `expected words` or the
//! source file the text must equal).
#[path = "../tests/support/mod.rs"]
mod support;

use atelier_fmt_ooxml::{
    DocxWriteOptions, NewDocument, W3cDateTime, SaveContext, import_docx, write_docx,
    write_new_docx,
};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use support::scratch::*;
use support::*;

fn save_context() -> SaveContext {
    SaveContext {
        now: W3cDateTime::utc(2026, 10, 14, 9, 30, 0).unwrap(),
        user: Some("Atelier".into()),
        application: Some("Atelier".into()),
        app_version: Some("0.1".into()),
        statistics: None,
    }
}

fn options() -> DocxWriteOptions {
    DocxWriteOptions {
        save: Some(save_context()),
        ..DocxWriteOptions::default()
    }
}

fn expected_file(dir: &Path, name: &str, doc: &atelier_text_model::rich::RichDocument) -> String {
    let file = format!("{name}.words.txt");
    fs::write(dir.join(&file), words(&all_text(doc)).join("\n")).unwrap();
    file
}

fn main() {
    let out = std::env::args().nth(1).expect("output directory");
    let out = Path::new(&out);
    for sub in ["roundtrip", "new", "modified", "word"] {
        fs::create_dir_all(out.join(sub)).unwrap();
    }
    let mut manifest = String::new();

    // Round trip of the corpus and of the built documents (no edit).
    let mut docs = corpus();
    docs.extend(built_documents());
    for (name, bytes) in &docs {
        let imp = import_docx(bytes).unwrap();
        let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
        let file = format!("{}.docx", name.replace(['/', '\\'], "_"));
        fs::write(out.join("roundtrip").join(&file), &w.bytes).unwrap();
        let source = format!("{}.source.docx", name.replace(['/', '\\'], "_"));
        fs::write(out.join("roundtrip").join(&source), bytes).unwrap();
        let _ = writeln!(manifest, "roundtrip\troundtrip/{file}\troundtrip/{source}");
    }

    // Documents created from scratch.
    for i in 0..120 {
        let built = scratch_document(i);
        let w = write_new_docx(&NewDocument::from_default(&built.doc), &options()).unwrap();
        let file = format!("{}.docx", built.name);
        fs::write(out.join("new").join(&file), &w.bytes).unwrap();
        let exp = expected_file(&out.join("new"), &built.name, &built.doc.document);
        let _ = writeln!(manifest, "new\tnew/{file}\tnew/{exp}");
    }

    // The same, edited by random operations.
    for i in 0..60 {
        let mut built = scratch_document(i * 2 + 1);
        let mut rng = Rng(0xABCD_0000 + i as u64);
        for _ in 0..(3 + i % 12) {
            mutate(&mut built.doc.document, &mut rng);
        }
        let w = write_new_docx(&NewDocument::from_default(&built.doc), &options()).unwrap();
        let name = format!("scratch-edited-{i:03}");
        let file = format!("{name}.docx");
        fs::write(out.join("modified").join(&file), &w.bytes).unwrap();
        let exp = expected_file(&out.join("modified"), &name, &built.doc.document);
        let _ = writeln!(manifest, "modified\tmodified/{file}\tmodified/{exp}");
    }

    // Imported documents, edited by random operations and written back.
    let stable = ["basic.docx", "libreoffice/rich.docx", "text", "tables"];
    for (name, bytes) in docs.iter().filter(|(n, _)| stable.contains(&n.as_str())) {
        for seed in 0..10u64 {
            let mut imp = import_docx(bytes).unwrap();
            let mut rng = Rng(0x1234_5678 + seed * 977);
            for _ in 0..(2 + seed) {
                mutate(&mut imp.document, &mut rng);
            }
            let w = write_docx(&imp, &options()).unwrap();
            let id = format!("{}-edited-{seed}", name.replace(['/', '\\', '.'], "_"));
            let file = format!("{id}.docx");
            fs::write(out.join("modified").join(&file), &w.bytes).unwrap();
            let exp = expected_file(&out.join("modified"), &id, &imp.document);
            let _ = writeln!(manifest, "modified\tmodified/{file}\tmodified/{exp}");
        }
    }

    // Files for the manual comparison in Word.
    for i in [0usize, 1, 2, 3, 5, 8, 10, 11] {
        let built = scratch_document(i);
        let w = write_new_docx(&NewDocument::from_default(&built.doc), &options()).unwrap();
        fs::write(out.join("word").join(format!("sample-{i:02}.docx")), &w.bytes).unwrap();
    }
    fs::write(out.join("manifest.tsv"), manifest).unwrap();
    println!("samples written under {}", out.display());
}
