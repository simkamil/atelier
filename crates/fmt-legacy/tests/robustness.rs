// SPDX-License-Identifier: AGPL-3.0-or-later
//! Limits and hostile input: every configured limit is enforced, and seeded
//! mutation, truncation and random-file campaigns never panic or loop.

mod common;

use atelier_cfb::{Compound, Limits as CfbLimits};
use atelier_fmt_legacy::doc::{DocError, DocLimits, import_doc, import_doc_with_limits};
use common::*;
use std::time::{Duration, Instant};

fn rich_spec() -> DocSpec {
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("Un texte \u{E9}crit "),
        PieceSpec::uni("en \u{65E5}\u{672C}\u{1F600}\r"),
        PieceSpec::ansi("Deuxi\u{E8}me paragraphe\r"),
    ]);
    spec.fonts = vec![FontSpec::new("Arial"), FontSpec::new("Times New Roman")];
    spec.styles = Some(vec![
        StdSpec::para(
            0,
            "Normal",
            0x0FFF,
            sprm_u16(0xA414, 100),
            sprm_u16(0x4A4F, 0),
        ),
        StdSpec::para(1, "Heading 1", 0, sprm_u8(0x2403, 1), sprm_u8(0x0835, 1)),
        StdSpec::chr(87, "Strong", 0x0FFF, sprm_u8(0x0836, 1)),
    ]);
    spec.chpx = vec![
        CharRunSpec {
            cp_start: 0,
            cp_end: 3,
            grpprl: cat(&[sprm_u8(0x0835, 1), sprm_u16(0x4A43, 28)]),
        },
        CharRunSpec {
            cp_start: 5,
            cp_end: 9,
            grpprl: cat(&[sprm_u16(0x4A30, 2), sprm_u32(0x6870, 0x0033_6699)]),
        },
    ];
    spec.papx = vec![
        ParaRunSpec {
            cp_start: 0,
            cp_end: 22,
            istd: 1,
            grpprl: cat(&[sprm_i16(0x840F, 720), sprm_var(0xC60D, &[0, 0])]),
        },
        ParaRunSpec {
            cp_start: 22,
            cp_end: 44,
            istd: 0,
            grpprl: sprm_u32(0x6646, 0),
        },
    ];
    spec.data = Some(vec![4, 0, 0x03, 0x24, 1, 0, 0, 0]);
    spec.section_ends = vec![44];
    spec.summary = Some(build_summary(&[
        Prop::CodePage(1252),
        Prop::Str(2, "Titre".into()),
        Prop::Time(12, 132_000_000_000_000_000),
    ]));
    spec
}

#[test]
fn every_limit_is_enforced() {
    let bytes = rich_spec().build();
    assert!(import_doc(&bytes).is_ok());
    let d = DocLimits::default();
    let cases: Vec<(&str, DocLimits)> = vec![
        (
            "text characters",
            DocLimits {
                max_text_chars: 5,
                ..d
            },
        ),
        ("piece count", DocLimits { max_pieces: 2, ..d }),
        (
            "FKP page count",
            DocLimits {
                max_fkp_pages: 0,
                ..d
            },
        ),
        ("style count", DocLimits { max_styles: 2, ..d }),
        ("font count", DocLimits { max_fonts: 1, ..d }),
        (
            "paragraph count",
            DocLimits {
                max_paragraphs: 1,
                ..d
            },
        ),
        ("character runs", DocLimits { max_runs: 1, ..d }),
        ("sprm count", DocLimits { max_sprms: 3, ..d }),
    ];
    for (limit, limits) in cases {
        match import_doc_with_limits(&bytes, limits) {
            Err(DocError::Limit(l)) => assert_eq!(l, limit),
            other => panic!("{limit}: {:?}", other.map(|_| ())),
        }
    }
    let small = DocLimits {
        cfb: CfbLimits {
            max_file_size: 1024,
            ..d.cfb
        },
        ..d
    };
    assert!(matches!(
        import_doc_with_limits(&bytes, small),
        Err(DocError::Cfb(_))
    ));
}

#[test]
fn absurd_declared_sizes_are_rejected_before_allocation() {
    let mut s = DocSpec::ansi("x\r").streams();
    // ccpText = 0xFFFF_FFF0 (FibRgLw index 3).
    let lw_at = 34 + 28 + 2;
    s.word[lw_at + 12..lw_at + 16].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    let bytes = cfb::Builder::new(
        3,
        vec![
            cfb::stream("WordDocument", &s.word),
            cfb::stream("0Table", &s.table),
        ],
    )
    .build()
    .bytes;
    assert_eq!(
        import_doc(&bytes).err(),
        Some(DocError::Limit("text characters"))
    );
    // A text length just under the limit but larger than the pieces: damage notice.
    let mut s = DocSpec::ansi("x\r").streams();
    s.word[lw_at + 12..lw_at + 16].copy_from_slice(&1_000_000u32.to_le_bytes());
    let bytes = cfb::Builder::new(
        3,
        vec![
            cfb::stream("WordDocument", &s.word),
            cfb::stream("0Table", &s.table),
        ],
    )
    .build()
    .bytes;
    let doc = import_doc(&bytes).unwrap();
    assert!(doc.notices.iter().any(|n| matches!(
        n,
        atelier_fmt_legacy::doc::ImportNotice::DamagedData {
            what: "main text shorter than declared",
            ..
        }
    )));
}

#[test]
fn large_document_imports_in_reasonable_time() {
    let mut text = String::new();
    for i in 0..20_000 {
        text.push_str(&format!("Paragraphe num\u{E9}ro {i} avec du texte.\r"));
    }
    let mut spec = DocSpec::ansi(&text);
    for i in 0..2_000usize {
        spec.chpx.push(CharRunSpec {
            cp_start: i * 40,
            cp_end: i * 40 + 5,
            grpprl: sprm_u8(0x0835, 1),
        });
    }
    let bytes = spec.build();
    let start = Instant::now();
    let doc = import_doc(&bytes).unwrap();
    assert_eq!(doc.paragraph_ids().len(), 20_000);
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
}

// ----- campaigns --------------------------------------------------------------------------------

fn rebuild(word: &[u8], table: &[u8], extra: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut children = vec![
        cfb::stream("WordDocument", word),
        cfb::stream("0Table", table),
        cfb::stream("1Table", table),
    ];
    for (n, d) in extra {
        children.push(cfb::stream(n, d));
    }
    cfb::Builder::new(3, children).build().bytes
}

fn mutate(rng: &mut Rng, data: &mut Vec<u8>) {
    if data.is_empty() {
        return;
    }
    let n = 1 + rng.below(4);
    for _ in 0..n {
        if data.is_empty() {
            return;
        }
        match rng.below(6) {
            0 => {
                let i = rng.below(data.len());
                data[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(data.len());
                data[i] = rng.next() as u8;
            }
            2 => {
                let i = rng.below(data.len().saturating_sub(3).max(1));
                let v = [
                    0u32,
                    1,
                    0xFF,
                    0x7FFF_FFFF,
                    0x8000_0000,
                    0xFFFF_FFFF,
                    0x200,
                    0x4000_0000,
                ][rng.below(8)];
                if i + 4 <= data.len() {
                    data[i..i + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            3 => {
                let i = rng.below(data.len());
                let l = rng.below(64).min(data.len() - i);
                data[i..i + l].fill(0);
            }
            4 => {
                let i = rng.below(data.len());
                let l = rng.below(32).min(data.len() - i);
                let chunk = data[i..i + l].to_vec();
                let j = rng.below(data.len());
                for (k, b) in chunk.iter().enumerate() {
                    if j + k < data.len() {
                        data[j + k] = *b;
                    }
                }
            }
            _ => {
                let keep = rng.below(data.len() + 1);
                data.truncate(keep);
            }
        }
    }
}

fn exercise(bytes: &[u8], counts: &mut (usize, usize)) {
    let start = Instant::now();
    match import_doc(bytes) {
        Ok(mut doc) => {
            counts.0 += 1;
            for id in doc.paragraph_ids().to_vec() {
                let _ = doc.resolved_paragraph(id);
            }
        }
        Err(_) => counts.1 += 1,
    }
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "iteration took {:?}",
        start.elapsed()
    );
}

type Extra = Vec<(String, Vec<u8>)>;

fn streams_of(bytes: &[u8]) -> (Vec<u8>, Vec<u8>, Extra) {
    let c = Compound::open(bytes, CfbLimits::default()).unwrap();
    let word = c.read("WordDocument").unwrap();
    let table = c.read("1Table").or_else(|_| c.read("0Table")).unwrap();
    let mut extra = Vec::new();
    for name in ["\u{5}SummaryInformation", "Data"] {
        if let Ok(d) = c.read(name) {
            extra.push((name.to_string(), d));
        }
    }
    (word, table, extra)
}

fn campaign(name: &str, bytes: &[u8], seed: u64, iterations: usize) {
    let (word, table, extra) = streams_of(bytes);
    // Reference files keep the table in 1Table: the FIB flag selects the right one.
    let mut rng = Rng(seed);
    let mut counts = (0usize, 0usize);
    for it in 0..iterations {
        let (mut w, mut t) = (word.clone(), table.clone());
        let mut e = extra.clone();
        match rng.below(8) {
            0..=2 => mutate(&mut rng, &mut w),
            3..=4 => mutate(&mut rng, &mut t),
            5 => {
                // The FIB and the first bytes of the table, where structure lives.
                let n = w.len().min(1024);
                let mut head = w[..n].to_vec();
                mutate(&mut rng, &mut head);
                w[..n.min(head.len())].copy_from_slice(&head[..n.min(head.len())]);
            }
            6 => {
                if let Some((_, d)) = e.first_mut() {
                    mutate(&mut rng, d);
                }
                mutate(&mut rng, &mut w);
            }
            _ => {
                // Whole-file mutation (mostly rejected by the container).
                let mut whole = rebuild(&w, &t, &e);
                mutate(&mut rng, &mut whole);
                exercise(&whole, &mut counts);
                continue;
            }
        }
        let file = rebuild(&w, &t, &e);
        exercise(&file, &mut counts);
        let _ = it;
    }
    assert!(counts.0 > 0, "{name}: no mutation was accepted");
    assert!(counts.1 > 0, "{name}: no mutation was rejected");
    println!("{name}: {} accepted, {} rejected", counts.0, counts.1);
}

#[test]
fn mutation_campaign_on_built_files() {
    campaign("rich", &rich_spec().build(), 0x5EED_0001, 3000);
    let mut small = DocSpec::ansi("a\rb\tc\u{0B}d\u{13}x\u{14}y\u{15}\r");
    small.chpx.push(CharRunSpec {
        cp_start: 0,
        cp_end: 2,
        grpprl: sprm_u8(0x0835, 1),
    });
    campaign("small", &small.build(), 0x5EED_0002, 3000);
    let mut fast = rich_spec();
    fast.use_table1 = true;
    fast.flags = 0x0004 | 0x0030;
    fast.pieces[1].prc = Some(sprm_u8(0x0836, 1));
    campaign("fast", &fast.build(), 0x5EED_0003, 3000);
}

#[test]
fn mutation_campaign_on_reference_files() {
    let files: [(&str, &[u8]); 4] = [
        (
            "text-accents",
            include_bytes!("../../../tests/doc-reference/text-accents.doc"),
        ),
        (
            "char-formatting",
            include_bytes!("../../../tests/doc-reference/char-formatting.doc"),
        ),
        (
            "para-formatting",
            include_bytes!("../../../tests/doc-reference/para-formatting.doc"),
        ),
        (
            "styles",
            include_bytes!("../../../tests/doc-reference/styles.doc"),
        ),
    ];
    for (i, (name, bytes)) in files.into_iter().enumerate() {
        campaign(name, bytes, 0x5EED_1000 + i as u64, 1500);
    }
}

#[test]
fn truncations_never_panic() {
    let bytes = rich_spec().build();
    let (word, table, extra) = streams_of(&bytes);
    let mut counts = (0, 0);
    // Every prefix of the structured part of each stream.
    for n in (0..word.len().min(2600)).step_by(7) {
        exercise(&rebuild(&word[..n], &table, &extra), &mut counts);
    }
    for n in 0..table.len() {
        exercise(&rebuild(&word, &table[..n], &extra), &mut counts);
    }
    for n in 0..bytes.len().min(3000) {
        exercise(&bytes[..n], &mut counts);
    }
    assert!(counts.0 > 0 && counts.1 > 0, "{counts:?}");
}

#[test]
fn random_streams_with_a_valid_fib_never_panic() {
    let mut rng = Rng(0x5EED_2000);
    let mut counts = (0, 0);
    for _ in 0..1500 {
        let mut word = vec![0u8; 512 + rng.below(4096)];
        for b in word.iter_mut() {
            *b = rng.next() as u8;
        }
        word[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
        word[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
        word[10] &= 0xFE; // keep fEncrypted clear (bit 0x100 is in byte 11)
        word[11] &= 0xFE;
        // Plausible counts so that the deeper parsers are reached.
        word[32..34].copy_from_slice(&14u16.to_le_bytes());
        word[62..64].copy_from_slice(&22u16.to_le_bytes());
        word[152..154].copy_from_slice(&93u16.to_le_bytes());
        let mut table = vec![0u8; 256 + rng.below(2048)];
        for b in table.iter_mut() {
            *b = rng.next() as u8;
        }
        // A syntactically valid Clx pointing at the start of the stream.
        let clx_at = 16usize;
        table[clx_at] = 2;
        let n = 1 + rng.below(3);
        let plc = 4 * (n + 1) + 8 * n;
        table[clx_at + 1..clx_at + 5].copy_from_slice(&(plc as u32).to_le_bytes());
        word[154 + 33 * 8..154 + 33 * 8 + 4].copy_from_slice(&(clx_at as u32).to_le_bytes());
        word[154 + 33 * 8 + 4..154 + 33 * 8 + 8].copy_from_slice(&((plc + 5) as u32).to_le_bytes());
        for i in 0..=n {
            let cp = (i * (1 + rng.below(40))) as u32;
            table[clx_at + 5 + 4 * i..clx_at + 9 + 4 * i].copy_from_slice(&cp.to_le_bytes());
        }
        let mut prev = 0u32;
        for i in 0..=n {
            let o = clx_at + 5 + 4 * i;
            let v = u32::from_le_bytes(table[o..o + 4].try_into().unwrap()).max(prev);
            table[o..o + 4].copy_from_slice(&v.to_le_bytes());
            prev = v;
        }
        let cc = prev.to_le_bytes();
        word[66 + 12..66 + 16].copy_from_slice(&cc);
        exercise(&rebuild(&word, &table, &[]), &mut counts);
    }
    assert!(counts.0 > 0, "{counts:?}");
}
