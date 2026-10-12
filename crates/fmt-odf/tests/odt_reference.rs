// SPDX-License-Identifier: AGPL-3.0-or-later
//! Import of the genuine LibreOffice corpus in `tests/odt-reference/`, compared
//! with expectations re-derived independently by `tools/generate-odt-reference.py`.

use atelier_fmt_odf::odt::{ImportedOdt, import_odt};
use atelier_text_model::rich::{LinkTarget, Node, NodeId};
use std::collections::BTreeMap;
use std::path::PathBuf;

// ---- a minimal JSON reader (the workspace has no external dependencies) ----

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn value(&mut self) -> Json {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut m = BTreeMap::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b'}' {
                        self.i += 1;
                        return Json::Obj(m);
                    }
                    if self.b[self.i] == b',' {
                        self.i += 1;
                        continue;
                    }
                    let Json::Str(k) = self.value() else {
                        panic!("key")
                    };
                    self.ws();
                    assert_eq!(self.b[self.i], b':');
                    self.i += 1;
                    m.insert(k, self.value());
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b']' {
                        self.i += 1;
                        return Json::Arr(v);
                    }
                    if self.b[self.i] == b',' {
                        self.i += 1;
                        continue;
                    }
                    v.push(self.value());
                }
            }
            b'"' => {
                self.i += 1;
                let mut out = Vec::new();
                while self.b[self.i] != b'"' {
                    if self.b[self.i] == b'\\' {
                        self.i += 1;
                        match self.b[self.i] {
                            b'n' => out.push(b'\n'),
                            b't' => out.push(b'\t'),
                            b'r' => out.push(b'\r'),
                            b'u' => {
                                let h =
                                    std::str::from_utf8(&self.b[self.i + 1..self.i + 5]).unwrap();
                                let c =
                                    char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap();
                                let mut buf = [0; 4];
                                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                                self.i += 4;
                            }
                            c => out.push(c),
                        }
                    } else {
                        out.push(self.b[self.i]);
                    }
                    self.i += 1;
                }
                self.i += 1;
                Json::Str(String::from_utf8(out).unwrap())
            }
            b'n' => {
                self.i += 4;
                Json::Null
            }
            b't' => {
                self.i += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Json::Bool(false)
            }
            _ => {
                let s = self.i;
                while self.i < self.b.len() && !b",]} \n\r\t".contains(&self.b[self.i]) {
                    self.i += 1;
                }
                Json::Num(
                    std::str::from_utf8(&self.b[s..self.i])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }
}

fn json(path: &PathBuf) -> Json {
    let bytes = std::fs::read(path).unwrap();
    Parser { b: &bytes, i: 0 }.value()
}

impl Json {
    fn get(&self, k: &str) -> &Json {
        match self {
            Json::Obj(m) => m.get(k).unwrap_or(&Json::Null),
            _ => panic!("not an object"),
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            Json::Null => &[],
            _ => panic!("not an array"),
        }
    }
    fn strings(&self) -> Vec<String> {
        self.arr().iter().map(|s| s.string().to_owned()).collect()
    }
    fn string(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
    fn int(&self) -> usize {
        match self {
            Json::Num(n) => *n as usize,
            other => panic!("not a number: {other:?}"),
        }
    }
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/odt-reference")
}

fn all_nodes(doc: &ImportedOdt, ids: Vec<NodeId>, out: &mut Vec<NodeId>) {
    for id in ids {
        out.push(id);
        if let Some(node) = doc.document.node(id) {
            all_nodes(doc, node.children(), out);
        }
    }
}

fn body_nodes(doc: &ImportedOdt) -> Vec<NodeId> {
    let mut out = Vec::new();
    let sections = doc.document.section_ids();
    for s in &sections {
        let Some(Node::Section(sec)) = doc.document.node(*s) else {
            panic!()
        };
        all_nodes(doc, sec.body.to_vec(), &mut out);
    }
    out
}

#[test]
fn every_reference_file_matches_its_independent_expectations() {
    let dir = corpus_dir();
    let expected = json(&dir.join("expected.json"));
    let manifest = json(&dir.join("manifest.json"));
    let Json::Obj(files) = &expected else {
        panic!()
    };
    assert!(files.len() >= 12, "the corpus must stay representative");
    let mut total_pages = 0;
    for (name, want) in files {
        let path = dir.join(name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            bytes.len(),
            manifest.get("files").get(name).get("bytes").int(),
            "{name} differs from the manifest (re-run tools/generate-odt-reference.py --check)"
        );
        let mut doc = import_odt(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        // Text, in order, including list items and table cells.
        let got: Vec<String> = doc
            .paragraph_ids()
            .iter()
            .map(|id| doc.document.paragraph(*id).unwrap().text.clone())
            .collect();
        assert_eq!(got, want.get("paragraphs").strings(), "{name}: paragraphs");
        // Headings.
        let headings: Vec<(usize, String)> = doc
            .headings
            .iter()
            .map(|(id, l)| {
                (
                    *l as usize,
                    doc.document.paragraph(*id).unwrap().text.clone(),
                )
            })
            .collect();
        let want_headings: Vec<(usize, String)> = want
            .get("headings")
            .arr()
            .iter()
            .map(|h| (h.arr()[0].int(), h.arr()[1].string().to_owned()))
            .collect();
        assert_eq!(headings, want_headings, "{name}: headings");
        // Structure counts.
        let nodes = body_nodes(&doc);
        let tables = nodes
            .iter()
            .filter(|id| matches!(doc.document.node(**id), Some(Node::Table(_))))
            .count();
        assert_eq!(tables, want.get("tables").int(), "{name}: tables");
        let mut images = 0;
        let mut links = Vec::new();
        let mut bookmarks = Vec::new();
        for id in &nodes {
            if let Some(Node::Paragraph(p)) = doc.document.node(*id) {
                images += p
                    .anchored
                    .iter()
                    .filter(|a| a.kind.starts_with("image/"))
                    .count();
                for l in &p.links {
                    links.push(match &l.target {
                        LinkTarget::External(u) => u.clone(),
                        LinkTarget::Bookmark(b) => format!("#{b}"),
                    });
                }
                bookmarks.extend(p.bookmarks.iter().map(|b| b.name.clone()));
            }
        }
        assert_eq!(images, want.get("images").int(), "{name}: images");
        assert_eq!(links, want.get("links").strings(), "{name}: links");
        bookmarks.sort();
        assert_eq!(
            bookmarks,
            want.get("bookmarks").strings(),
            "{name}: bookmarks"
        );
        // Sections follow the master-page changes.
        let changes = want.get("masterPageChanges").arr().len();
        let sections = doc.document.section_ids().len();
        assert_eq!(
            sections,
            if changes == 0 { 1 } else { 1 + changes },
            "{name}: sections"
        );
        // Metadata.
        assert_eq!(
            doc.metadata.title.as_deref(),
            match want.get("title") {
                Json::Str(s) => Some(s.as_str()),
                _ => None,
            }
        );
        assert_eq!(
            doc.metadata.keywords,
            want.get("keywords").strings(),
            "{name}: keywords"
        );
        // Every paragraph resolves through the style engine.
        for id in doc.paragraph_ids().to_vec() {
            doc.resolved_paragraph(id)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        total_pages += want.get("pageCount").int();
    }
    assert!(total_pages >= files.len());
}

#[test]
fn reference_specifics() {
    let dir = corpus_dir();
    let load = |n: &str| import_odt(&std::fs::read(dir.join(n)).unwrap()).unwrap();
    // Character formatting survives LibreOffice's own automatic styles.
    let doc = load("char-format.odt");
    let p = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    let has = |f: &dyn Fn(&atelier_text_model::rich::CharAttrs) -> bool| {
        p.runs.iter().any(|r| f(&r.attrs))
    };
    assert!(has(&|a| a.bold == Some(true)));
    assert!(has(&|a| a.italic == Some(true)));
    assert!(has(&|a| a.strike == Some(true)));
    assert!(has(&|a| {
        a.underline
            .is_some_and(|u| u != atelier_text_model::rich::Underline::None)
    }));
    assert!(has(&|a| a.color
        == Some(atelier_text_model::rich::Color {
            r: 0xcc,
            g: 0,
            b: 0
        })));
    assert!(has(&|a| a.highlight
        == Some(atelier_text_model::rich::Color {
            r: 255,
            g: 255,
            b: 0
        })));
    assert!(has(
        &|a| a.font.as_deref() == Some("Liberation Mono") && a.size_half_points == Some(20)
    ));
    assert!(has(&|a| a.size_half_points == Some(40)));
    // Headers and footers carry fields.
    let doc = load("header-footer.odt");
    let Some(Node::Section(s)) = doc.document.node(doc.document.section_ids()[0]) else {
        panic!()
    };
    let (_, footer) = s.footers[0];
    let Some(Node::HeaderFooter(f)) = doc.document.node(footer) else {
        panic!()
    };
    let para = doc.document.paragraph(f.blocks.get(0).unwrap()).unwrap();
    assert_eq!(para.text, "Page 1 sur 1");
    assert_eq!(para.fields.len(), 2);
    assert_eq!(doc.sections[0].page.width, Some(11906));
    // Lists keep levels.
    let doc = load("lists.odt");
    let levels: Vec<u8> = doc
        .paragraph_ids()
        .iter()
        .filter_map(|id| {
            doc.document
                .paragraph(*id)
                .unwrap()
                .props
                .list
                .map(|l| l.level)
        })
        .collect();
    assert_eq!(levels, vec![0, 1, 2, 1, 0, 0, 0, 1, 1, 0]);
    assert_eq!(doc.lists.len(), 2);
    // Merged cells.
    let doc = load("table-merge.odt");
    let spans: Vec<(usize, usize)> = body_nodes(&doc)
        .iter()
        .filter_map(|id| match doc.document.node(*id) {
            Some(Node::Cell(c)) => Some((c.col_span, c.row_span)),
            _ => None,
        })
        .filter(|s| *s != (1, 1))
        .collect();
    assert_eq!(spans, vec![(2, 1), (1, 2)]);
    // Image bytes are kept untouched and the alternative text is read.
    let doc = load("image.odt");
    let img = &doc.images[0];
    assert_eq!(img.title.as_deref(), Some("Dégradé"));
    let para = doc.document.paragraph(img.paragraph).unwrap();
    let obj = para.anchored.iter().find(|a| a.id == img.object).unwrap();
    assert_eq!(obj.kind, "image/png");
    assert_eq!(&obj.data[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn flat_libreoffice_document_equals_its_package_counterpart() {
    let dir = corpus_dir();
    let flat = import_odt(&std::fs::read(dir.join("table-merge.fodt")).unwrap()).unwrap();
    let packed = import_odt(&std::fs::read(dir.join("table-merge.odt")).unwrap()).unwrap();
    let text = |d: &ImportedOdt| -> Vec<String> {
        d.paragraph_ids()
            .iter()
            .map(|id| d.document.paragraph(*id).unwrap().text.clone())
            .collect()
    };
    assert_eq!(text(&flat), text(&packed));
    assert_eq!(body_nodes(&flat).len(), body_nodes(&packed).len());
}

/// Local, out-of-repository check: set `ATELIER_ODT_LOCAL_DIR` to a directory
/// holding arbitrary `.odt` / `.ott` / `.fodt` files (for example the templates
/// shipped with an office suite) and run with `--ignored --nocapture`.
#[test]
#[ignore = "needs ATELIER_ODT_LOCAL_DIR; files stay outside the repository"]
fn local_directory_imports_without_error() {
    let Ok(dir) = std::env::var("ATELIER_ODT_LOCAL_DIR") else {
        panic!("ATELIER_ODT_LOCAL_DIR is not set");
    };
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "odt" | "ott" | "fodt"))
            {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(std::path::Path::new(&dir), &mut files);
    files.sort();
    let (mut ok, mut failed) = (0, 0);
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        if bytes.is_empty() {
            println!("skip {} (empty file)", f.display());
            continue;
        }
        match import_odt(&bytes) {
            Ok(mut doc) => {
                ok += 1;
                for id in doc.paragraph_ids().to_vec() {
                    doc.resolved_paragraph(id)
                        .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
                }
                println!(
                    "ok   {:<60} paragraphs={} sections={} notices={}",
                    f.file_name().unwrap().to_string_lossy(),
                    doc.paragraph_ids().len(),
                    doc.sections.len(),
                    doc.notices.len()
                );
            }
            Err(e) => {
                failed += 1;
                println!("FAIL {}: {e}", f.display());
            }
        }
    }
    println!("{} files: {ok} imported, {failed} refused", files.len());
    assert_eq!(failed, 0);
}
