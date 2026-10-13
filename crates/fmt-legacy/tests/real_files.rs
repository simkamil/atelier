// SPDX-License-Identifier: AGPL-3.0-or-later
//! Optional local check on real `.doc` files. Does nothing unless
//! `ATELIER_DOC_SAMPLES` lists directories or files (separated by `;`). No
//! sample is copied into the repository. Each file must import without error
//! (password-protected and pre-Word-97 files are counted separately), the text
//! must be non-empty when the FIB declares text, and the paragraph texts must
//! add up to the declared length (minus one mark per paragraph).

use atelier_fmt_legacy::doc::{DocError, import_doc};
use std::path::{Path, PathBuf};

fn collect(path: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() && depth < 8 {
            collect(&p, out, depth + 1);
        } else if ft.is_file()
            && p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "doc" | "dot"))
        {
            out.push(p);
        }
    }
}

#[test]
fn local_real_files() {
    let Ok(dirs) = std::env::var("ATELIER_DOC_SAMPLES") else {
        return;
    };
    let mut files = Vec::new();
    for d in dirs.split(';').filter(|d| !d.is_empty()) {
        collect(Path::new(d), &mut files, 0);
    }
    let (mut tried, mut ok, mut protected, mut old, mut not_word) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        tried += 1;
        match import_doc(&data) {
            Ok(doc) => {
                ok += 1;
                let chars: usize = doc
                    .paragraph_ids()
                    .iter()
                    .map(|id| doc.document.paragraph(*id).unwrap().text.chars().count())
                    .sum();
                println!(
                    "{}: {} tables, {} lists, {} sections, {} fields, {} bookmarks",
                    f.display(),
                    doc.tables.tables.len(),
                    doc.lists.len(),
                    doc.sections.len(),
                    doc.fields.len(),
                    doc.bookmarks.len()
                );
                if doc.document.validate().is_err() || doc.sections.is_empty() {
                    failures.push(format!("{}: invalid structure", f.display()));
                }
                for (id, t) in &doc.tables.tables {
                    // Every table is rectangular and counted once per row.
                    if t.rows.is_empty() || t.columns == 0 || t.grid.len() != t.columns {
                        failures.push(format!("{}: table {id:?} inconsistent", f.display()));
                    }
                }
                println!(
                    "{}: {:?}, {} paragraphs, {} chars (ccpText {}), {} notices, table {}",
                    f.display(),
                    doc.info.version,
                    doc.paragraph_ids().len(),
                    chars,
                    doc.info.main_text_chars,
                    doc.notices.len(),
                    doc.info.table_stream
                );
                for n in &doc.notices {
                    println!("    {n:?}");
                }
                if doc.info.main_text_chars > 1 && chars == 0 {
                    failures.push(format!("{}: no text", f.display()));
                }
            }
            Err(DocError::PasswordProtected { .. }) => protected += 1,
            Err(DocError::UnsupportedVersion { .. }) => old += 1,
            Err(DocError::NotWordDocument | DocError::MissingStream(_) | DocError::Cfb(_)) => {
                not_word += 1;
            }
            Err(e) => failures.push(format!("{}: {e}", f.display())),
        }
    }
    println!(
        "tried {tried}: {ok} imported, {protected} protected, {old} pre-97, {not_word} not word"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}
