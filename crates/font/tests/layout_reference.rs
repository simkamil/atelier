// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, GlyphCluster};
use std::collections::BTreeMap;
use std::path::Path;

const REFERENCE: &str = include_str!("../../../tests/font-layout-reference/layout.tsv");

#[test]
fn all_embedded_faces_match_independent_harfbuzz_latin_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut grouped = BTreeMap::<&str, Vec<(&str, &str, usize)>>::new();
    for (line_number, line) in REFERENCE.lines().enumerate() {
        let mut columns = line.splitn(3, '\t');
        let font = columns.next().unwrap();
        let text = columns.next().unwrap();
        let expected = columns.next().unwrap();
        grouped
            .entry(font)
            .or_default()
            .push((text, expected, line_number + 1));
    }
    assert_eq!(grouped.len(), 20);
    let mut cases = 0;
    for (font, records) in grouped {
        let bytes = std::fs::read(root.join(font)).unwrap();
        let face = FontFace::parse(&bytes).unwrap();
        let layout = face.layout().unwrap_or_else(|e| panic!("{font}: {e:?}"));
        for (codepoints, expected, line) in records {
            let chars: Vec<char> = codepoints
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| char::from_u32(u32::from_str_radix(s, 16).unwrap()).unwrap())
                .collect();
            let input: Vec<_> = chars
                .iter()
                .enumerate()
                .map(|(i, &c)| GlyphCluster {
                    glyph_id: face.glyph_index(c),
                    cluster: i..i + 1,
                })
                .collect();
            let output = layout
                .apply_ligatures(&input)
                .unwrap_or_else(|e| panic!("line {line}: {e:?}"));
            let positions = layout
                .pair_adjustments(&output)
                .unwrap_or_else(|e| panic!("line {line}: {e:?}"));
            let actual = output
                .iter()
                .zip(positions)
                .map(|(g, p)| {
                    let advance =
                        i32::from(face.horizontal_metric(g.glyph_id).unwrap().advance_width);
                    format!(
                        "{},{},{},{},{},{},{}",
                        g.glyph_id,
                        g.cluster.start,
                        g.cluster.end,
                        p.x_placement,
                        p.y_placement,
                        advance + p.x_advance,
                        p.y_advance
                    )
                })
                .collect::<Vec<_>>()
                .join(";");
            assert_eq!(
                actual, expected,
                "{font} corpus line {line}, input {codepoints}"
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 54_716);
}
