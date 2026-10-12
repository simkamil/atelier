// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(clippy::field_reassign_with_default)]
//! Fixed-seed mutation and truncation campaign: arbitrary input must give a
//! typed error or an import with notices, never a panic or an endless loop.

use atelier_fmt_odf::odt::{OdtLimits, import_odt, import_odt_with_limits};
use atelier_zip::{Archive, Limits, Method, WriteOptions, Writer};
use std::path::PathBuf;

/// xorshift64*: small, deterministic, dependency free.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn corpus(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/odt-reference")
            .join(name),
    )
    .unwrap()
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let archive = Archive::open(bytes, Limits::default()).unwrap();
    archive
        .entries()
        .iter()
        .map(|e| (e.name().to_owned(), e.read_all().unwrap()))
        .collect()
}

fn rebuild(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = Writer::new(WriteOptions::default());
    for (n, b) in parts {
        if n.ends_with('/') {
            continue;
        }
        w.add(n, b, Method::Deflate).unwrap();
    }
    w.finish().unwrap()
}

fn mutate(rng: &mut Rng, data: &mut Vec<u8>) {
    for _ in 0..=rng.below(4) {
        if data.is_empty() {
            return;
        }
        match rng.below(7) {
            0 => {
                let i = rng.below(data.len());
                data[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(data.len());
                data.remove(i);
            }
            2 => {
                let i = rng.below(data.len());
                let j = rng.below(data.len());
                data.swap(i, j);
            }
            3 => {
                let len = rng.below(data.len());
                data.truncate(len);
            }
            4 => {
                // Duplicate a slice.
                let i = rng.below(data.len());
                let j = (i + rng.below(64)).min(data.len());
                let piece = data[i..j].to_vec();
                let at = rng.below(data.len());
                data.splice(at..at, piece);
            }
            5 => {
                // Overwrite with an interesting byte.
                let i = rng.below(data.len());
                data[i] = [b'<', b'>', b'&', b'"', 0, 0xff, b'/'][rng.below(7)];
            }
            _ => {
                // Replace an attribute-looking number.
                if let Some(p) = data.windows(2).position(|w| w == b"=\"") {
                    let at = (p + 2).min(data.len());
                    data.splice(at..at, *b"99999999999");
                }
            }
        }
    }
}

fn tight() -> OdtLimits {
    let mut l = OdtLimits::default();
    l.max_table_cells = 10_000;
    l.max_text_bytes = 1 << 20;
    l.max_part_bytes = 4 << 20;
    l
}

#[test]
fn xml_mutation_campaign_never_panics() {
    let names = [
        "table-merge.odt",
        "lists.odt",
        "header-footer.odt",
        "image.odt",
        "char-format.odt",
        "sections-pages.odt",
    ];
    let mut rng = Rng(0x00C0_FFEE_D15E_A5E5);
    let mut ok = 0;
    let mut err = 0;
    for name in names {
        let parts = entries(&corpus(name));
        for _ in 0..120 {
            let mut parts = parts.clone();
            let victim = rng.below(parts.len());
            let name_of = parts[victim].0.clone();
            if name_of == "mimetype" || name_of.starts_with("Pictures/") && rng.below(2) == 0 {
                continue;
            }
            mutate(&mut rng, &mut parts[victim].1);
            match import_odt_with_limits(&rebuild(&parts), tight()) {
                Ok(_) => ok += 1,
                Err(_) => err += 1,
            }
        }
    }
    assert!(
        ok > 50 && err > 20,
        "campaign should reach both outcomes ({ok} ok, {err} err)"
    );
}

#[test]
fn raw_zip_mutations_and_truncations_never_panic() {
    let mut rng = Rng(0xDEAD_BEEF_0BAD_CAFE);
    for name in ["text-accents.odt", "nested-table.odt", "multi-page.odt"] {
        let original = corpus(name);
        for _ in 0..200 {
            let mut data = original.clone();
            mutate(&mut rng, &mut data);
            let _ = import_odt_with_limits(&data, tight());
        }
        // Every prefix length class, including the empty file.
        for cut in (0..original.len()).step_by(original.len() / 150 + 1) {
            let _ = import_odt_with_limits(&original[..cut], tight());
        }
    }
}

#[test]
fn flat_document_mutations_never_panic() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let original = corpus("table-merge.fodt");
    for _ in 0..300 {
        let mut data = original.clone();
        mutate(&mut rng, &mut data);
        let _ = import_odt_with_limits(&data, tight());
    }
    for cut in (0..original.len()).step_by(original.len() / 200 + 1) {
        let _ = import_odt_with_limits(&original[..cut], tight());
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut rng = Rng(42);
    for len in [0usize, 1, 3, 30, 300, 3000] {
        for _ in 0..40 {
            let data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let _ = import_odt(&data);
            let mut xmlish = b"<office:document xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\">".to_vec();
            xmlish.extend(&data);
            let _ = import_odt(&xmlish);
        }
    }
}

#[test]
fn pathological_structures_are_bounded() {
    let ns = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\"";
    let wrap = |body: String| {
        format!(
            "<office:document {ns}><office:body><office:text>{body}</office:text></office:body></office:document>"
        )
    };
    // Deep span nesting: bounded by the XML depth limit, or imported.
    for depth in [10usize, 200, 5000] {
        let mut b = String::from("<text:p>");
        b.push_str(&"<text:span>".repeat(depth));
        b.push('x');
        b.push_str(&"</text:span>".repeat(depth));
        b.push_str("</text:p>");
        let _ = import_odt(wrap(b).as_bytes());
    }
    // Deep sections, tables in tables.
    for depth in [20usize, 100, 3000] {
        let b = format!(
            "{}<text:p>x</text:p>{}",
            "<text:section>".repeat(depth),
            "</text:section>".repeat(depth)
        );
        let _ = import_odt(wrap(b).as_bytes());
        let t = format!(
            "{}<text:p>x</text:p>{}",
            "<table:table><table:table-row><table:table-cell>".repeat(depth),
            "</table:table-cell></table:table-row></table:table>".repeat(depth)
        );
        let _ = import_odt(wrap(t).as_bytes());
    }
    // Many tiny paragraphs and style cycles.
    let many = "<text:p/>".repeat(50_000);
    let doc = import_odt(wrap(many).as_bytes()).unwrap();
    assert_eq!(doc.paragraph_ids().len(), 50_000);
    let cyc = format!(
        "<office:document {ns}><office:styles><style:style style:name=\"A\" style:family=\"paragraph\" style:parent-style-name=\"A\"/></office:styles><office:body><office:text><text:p text:style-name=\"A\">x</text:p></office:text></office:body></office:document>"
    );
    import_odt(cyc.as_bytes()).unwrap();
}
