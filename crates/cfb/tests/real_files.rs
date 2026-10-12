// SPDX-License-Identifier: AGPL-3.0-or-later
//! Optional local check on real files. Does nothing unless `ATELIER_CFB_SAMPLES`
//! lists directories (separated by `;`) to scan for `.doc`, `.xls`, `.ppt`,
//! `.msi`, `.dot`, `.msg` files. No sample is ever copied into the repository.

use atelier_cfb::{Compound, EntryKind, Limits, is_cfb};
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() && depth < 8 {
            collect(&p, out, depth + 1);
        } else if ft.is_file() {
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .map(str::to_ascii_lowercase);
            if matches!(
                ext.as_deref(),
                Some("doc" | "xls" | "ppt" | "msi" | "dot" | "msg")
            ) {
                out.push(p);
            }
        }
    }
}

#[test]
fn local_real_files() {
    let Ok(dirs) = std::env::var("ATELIER_CFB_SAMPLES") else {
        return;
    };
    let mut files = Vec::new();
    for d in dirs.split(';').filter(|d| !d.is_empty()) {
        collect(Path::new(d), &mut files, 0);
    }
    let limits = Limits {
        max_stream_size: 512 * 1024 * 1024,
        max_total_read: u64::MAX,
        ..Limits::default()
    };
    let (mut tried, mut not_cfb, mut opened, mut failed_open) = (0, 0, 0, 0);
    let (mut streams, mut stream_errors, mut anomalies, mut bytes) = (0u64, 0u64, 0usize, 0u64);
    for f in files {
        let Ok(data) = std::fs::read(&f) else {
            continue;
        };
        tried += 1;
        if !is_cfb(&data) {
            not_cfb += 1;
            continue;
        }
        match Compound::open(&data, limits) {
            Err(e) => {
                failed_open += 1;
                eprintln!("OPEN FAILED {}: {e}", f.display());
            }
            Ok(c) => {
                opened += 1;
                anomalies += c.anomalies().len();
                for l in c.list() {
                    if l.entry.kind() == EntryKind::Stream {
                        streams += 1;
                        match c.read_entry(l.entry) {
                            Ok(v) => bytes += v.len() as u64,
                            Err(e) => {
                                stream_errors += 1;
                                eprintln!("STREAM {} {}: {e}", f.display(), l.path);
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "real files: tried {tried}, not CFB {not_cfb}, opened {opened}, open failed {failed_open}, \
         streams {streams}, stream errors {stream_errors}, anomalies {anomalies}, bytes read {bytes}"
    );
    assert_eq!(failed_open, 0);
    assert_eq!(stream_errors, 0);
}
