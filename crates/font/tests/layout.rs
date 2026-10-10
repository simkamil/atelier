// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, GlyphAdjustment, GlyphCluster, LayoutError, LayoutLimits};

const BASE: &[u8] = include_bytes!("fixtures/font-0-12.font");

fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_be_bytes());
}
fn gsub_liga(flags: u16) -> Vec<u8> {
    let mut lig = vec![0; 8];
    put16(&mut lig, 0, 1);
    put16(&mut lig, 2, 18);
    put16(&mut lig, 4, 1);
    put16(&mut lig, 6, 8);
    lig.extend_from_slice(&[0, 1, 0, 4, 0, 4, 0, 2, 0, 3]);
    lig.extend_from_slice(&[0, 1, 0, 1, 0, 1]);

    let mut script = vec![
        0, 1, b'l', b'a', b't', b'n', 0, 8, 0, 4, 0, 0, 0, 0, 0xff, 0xff, 0, 1, 0, 0,
    ];
    let mut feature = vec![0, 1, b'l', b'i', b'g', b'a', 0, 8, 0, 0, 0, 1, 0, 0];
    let mut lookup = vec![0, 1, 0, 4, 0, 4];
    lookup.extend_from_slice(&flags.to_be_bytes());
    lookup.extend_from_slice(&[0, 1, 0, 8]);
    lookup.extend_from_slice(&lig);
    let script_off = 10usize;
    let feature_off = script_off + script.len();
    let lookup_off = feature_off + feature.len();
    let mut out = vec![0; 10];
    put16(&mut out, 0, 1);
    put16(&mut out, 4, script_off as u16);
    put16(&mut out, 6, feature_off as u16);
    put16(&mut out, 8, lookup_off as u16);
    out.append(&mut script);
    out.append(&mut feature);
    out.append(&mut lookup);
    out
}
fn gpos_pair(x_advance: i16) -> Vec<u8> {
    let mut pair = vec![0, 1, 0, 18, 0, 4, 0, 0, 0, 1, 0, 12];
    pair.extend_from_slice(&[0, 1, 0, 3]);
    pair.extend_from_slice(&x_advance.to_be_bytes());
    pair.extend_from_slice(&[0, 1, 0, 1, 0, 1]);
    let script = vec![
        0, 1, b'l', b'a', b't', b'n', 0, 8, 0, 4, 0, 0, 0, 0, 0xff, 0xff, 0, 1, 0, 0,
    ];
    let feature = vec![0, 1, b'k', b'e', b'r', b'n', 0, 8, 0, 0, 0, 1, 0, 0];
    let mut lookup = vec![0, 1, 0, 4, 0, 2, 0, 0, 0, 1, 0, 8];
    lookup.extend_from_slice(&pair);
    let script_off = 10usize;
    let feature_off = script_off + script.len();
    let lookup_off = feature_off + feature.len();
    let mut out = vec![0; 10];
    put16(&mut out, 0, 1);
    put16(&mut out, 4, script_off as u16);
    put16(&mut out, 6, feature_off as u16);
    put16(&mut out, 8, lookup_off as u16);
    out.extend_from_slice(&script);
    out.extend_from_slice(&feature);
    out.extend_from_slice(&lookup);
    out
}
fn legacy_kern(x_advance: i16) -> Vec<u8> {
    let mut b = vec![0, 0, 0, 1, 0, 0, 0, 20, 0, 6, 0, 0, 0, 0, 0, 1, 0, 3];
    b.extend_from_slice(&x_advance.to_be_bytes());
    b
}
fn gdef_mark() -> Vec<u8> {
    let mut b = vec![0; 12];
    put16(&mut b, 0, 1);
    put16(&mut b, 4, 12);
    b.extend_from_slice(&[0, 2, 0, 1, 0, 2, 0, 2, 0, 3]);
    b
}
fn with_tables(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let old_count = usize::from(u16::from_be_bytes([BASE[4], BASE[5]]));
    let old_end = 12 + old_count * 16;
    let mut entries: Vec<([u8; 4], Vec<u8>)> = (0..old_count)
        .map(|i| {
            let p = 12 + i * 16;
            let tag = BASE[p..p + 4].try_into().unwrap();
            let off = u32::from_be_bytes(BASE[p + 8..p + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(BASE[p + 12..p + 16].try_into().unwrap()) as usize;
            (tag, BASE[off..off + len].to_vec())
        })
        .collect();
    entries.extend(tables.iter().map(|(tag, data)| (**tag, data.clone())));
    let dir_end = 12 + entries.len() * 16;
    let mut out = vec![0; dir_end];
    out[..12].copy_from_slice(&BASE[..12]);
    put16(&mut out, 4, entries.len() as u16);
    for (i, (tag, data)) in entries.iter().enumerate() {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let off = out.len();
        let p = 12 + i * 16;
        out[p..p + 4].copy_from_slice(tag);
        let end = off + data.len();
        out.extend_from_slice(data);
        put16(&mut out, p + 4, 0);
        out[p + 8..p + 12].copy_from_slice(&(off as u32).to_be_bytes());
        out[p + 12..p + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        let _ = end;
    }
    let _ = old_end;
    out
}

#[test]
fn ligature_merges_components_and_ignore_marks_keeps_the_mark() {
    let font = with_tables(&[(b"GSUB", gsub_liga(0))]);
    let face = FontFace::parse(&font).unwrap();
    let layout = face.layout().unwrap();
    let input = [
        GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        },
        GlyphCluster {
            glyph_id: 3,
            cluster: 1..2,
        },
    ];
    assert_eq!(
        layout.apply_ligatures(&input).unwrap(),
        [GlyphCluster {
            glyph_id: 4,
            cluster: 0..2
        }]
    );

    let font = with_tables(&[(b"GSUB", gsub_liga(8)), (b"GDEF", gdef_mark())]);
    let face = FontFace::parse(&font).unwrap();
    let layout = face.layout().unwrap();
    let input = [
        GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        },
        GlyphCluster {
            glyph_id: 2,
            cluster: 1..2,
        },
        GlyphCluster {
            glyph_id: 3,
            cluster: 2..3,
        },
    ];
    assert_eq!(
        layout.apply_ligatures(&input).unwrap(),
        [
            GlyphCluster {
                glyph_id: 4,
                cluster: 0..3
            },
            GlyphCluster {
                glyph_id: 2,
                cluster: 1..2
            }
        ]
    );
}

#[test]
fn gpos_kern_feature_takes_precedence_over_legacy_kern() {
    let font = with_tables(&[(b"GPOS", gpos_pair(-80)), (b"kern", legacy_kern(-30))]);
    let face = FontFace::parse(&font).unwrap();
    let layout = face.layout().unwrap();
    let input = [
        GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        },
        GlyphCluster {
            glyph_id: 3,
            cluster: 1..2,
        },
    ];
    assert_eq!(
        layout.pair_adjustments(&input).unwrap(),
        [
            GlyphAdjustment {
                x_advance: -80,
                ..Default::default()
            },
            GlyphAdjustment::default()
        ]
    );
}

#[test]
fn absent_tables_are_identity_and_input_and_work_limits_are_checked() {
    let face = FontFace::parse(BASE).unwrap();
    let layout = face.layout().unwrap();
    assert_eq!(layout.apply_ligatures(&[]).unwrap(), []);
    assert_eq!(layout.pair_adjustments(&[]).unwrap(), []);
    assert_eq!(
        layout.apply_ligatures(&[GlyphCluster {
            glyph_id: 1,
            cluster: 2..2
        }]),
        Err(LayoutError::InvalidClusterRange)
    );
    let one = LayoutLimits {
        work_units: 1,
        ..LayoutLimits::default()
    };
    let layout = face.layout_with_limits(one).unwrap();
    let input = [GlyphCluster {
        glyph_id: 1,
        cluster: 0..1,
    }];
    assert_eq!(layout.apply_ligatures(&input).unwrap(), input);
    let zero = LayoutLimits {
        work_units: 0,
        ..one
    };
    let layout = face.layout_with_limits(zero).unwrap();
    assert!(matches!(
        layout.apply_ligatures(&input),
        Err(LayoutError::LimitExceeded(_))
    ));
}
