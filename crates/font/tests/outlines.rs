// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, OutlineError, OutlineLimits, embedded::EMBEDDED_FONTS};

#[test]
fn every_glyph_in_embedded_faces_decodes_without_panicking_or_partial_output() {
    for font in EMBEDDED_FONTS {
        let face = FontFace::parse(font.data).unwrap_or_else(|e| panic!("{}: {e}", font.family));
        for gid in 0..face.metrics().number_of_glyphs {
            face.outline(gid)
                .unwrap_or_else(|e| panic!("{} glyph {gid}: {e}", font.family));
        }
    }
}

#[test]
fn invalid_glyph_and_resource_budgets_return_typed_errors() {
    let font = EMBEDDED_FONTS
        .iter()
        .find(|f| {
            f.family == "Carlito"
                && f.weight == 400
                && f.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap();
    let face = FontFace::parse(font.data).unwrap();
    assert!(matches!(
        face.outline(face.metrics().number_of_glyphs),
        Err(OutlineError::InvalidGlyph)
    ));

    let a = face.glyph_index('A');
    assert!(matches!(
        face.outline_with_limits(
            a,
            OutlineLimits {
                max_points: 0,
                ..OutlineLimits::default()
            }
        ),
        Err(OutlineError::LimitExceeded("points"))
    ));
    assert!(matches!(
        face.outline_with_limits(
            a,
            OutlineLimits {
                max_depth: 65,
                ..OutlineLimits::default()
            }
        ),
        Err(OutlineError::LimitExceeded("public depth cap"))
    ));
}

#[test]
fn composites_fit_a_zero_component_budget_only_when_glyph_is_simple() {
    let font = EMBEDDED_FONTS
        .iter()
        .find(|f| {
            f.family == "Carlito"
                && f.weight == 400
                && f.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap();
    let face = FontFace::parse(font.data).unwrap();
    let mut saw_component = false;
    for gid in 0..face.metrics().number_of_glyphs {
        if matches!(
            face.outline_with_limits(
                gid,
                OutlineLimits {
                    max_components: 0,
                    ..OutlineLimits::default()
                }
            ),
            Err(OutlineError::LimitExceeded("components"))
        ) {
            saw_component = true;
            break;
        }
    }
    assert!(
        saw_component,
        "the reference font should exercise composite expansion"
    );
}

#[test]
fn corrupted_loca_and_composite_cycles_fail_cleanly() {
    let original = EMBEDDED_FONTS
        .iter()
        .find(|f| {
            f.family == "Carlito"
                && f.weight == 400
                && f.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap();
    let mut bytes = original.data.to_vec();
    let loca = table_range(&bytes, *b"loca");
    let face = FontFace::parse(&bytes).unwrap();
    if face.metrics().index_to_loc_format == 0 {
        bytes[loca.0..loca.0 + 2].copy_from_slice(&u16::MAX.to_be_bytes());
    } else {
        bytes[loca.0..loca.0 + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    }
    let malformed = FontFace::parse(&bytes).unwrap();
    assert!(matches!(
        malformed.outline(0),
        Err(OutlineError::InvalidData("loca offsets"))
    ));

    let mut bytes = original.data.to_vec();
    let face = FontFace::parse(&bytes).unwrap();
    let loca_range = table_range(&bytes, *b"loca");
    let glyf_range = table_range(&bytes, *b"glyf");
    let glyph_count = usize::from(face.metrics().number_of_glyphs);
    let offset = |index: usize| -> usize {
        if face.metrics().index_to_loc_format == 0 {
            usize::from(u16::from_be_bytes(
                bytes[loca_range.0 + index * 2..loca_range.0 + index * 2 + 2]
                    .try_into()
                    .unwrap(),
            )) * 2
        } else {
            u32::from_be_bytes(
                bytes[loca_range.0 + index * 4..loca_range.0 + index * 4 + 4]
                    .try_into()
                    .unwrap(),
            ) as usize
        }
    };
    let composite = (0..glyph_count)
        .find(|&gid| {
            let start = offset(gid);
            let end = offset(gid + 1);
            end.saturating_sub(start) >= 14
                && i16::from_be_bytes(
                    bytes[glyf_range.0 + start..glyf_range.0 + start + 2]
                        .try_into()
                        .unwrap(),
                ) < 0
        })
        .expect("Carlito contains a composite glyph");
    let start = offset(composite);
    let glyph_id_position = glyf_range.0 + start + 12;
    bytes[glyph_id_position..glyph_id_position + 2]
        .copy_from_slice(&(composite as u16).to_be_bytes());
    let cyclic = FontFace::parse(&bytes).unwrap();
    assert_eq!(
        cyclic.outline(composite as u16).unwrap_err(),
        OutlineError::Cycle
    );
}

#[test]
fn malformed_simple_flags_and_truncated_coordinates_return_typed_errors() {
    let original = carlito_regular();
    let mut bytes = original.data.to_vec();
    let face = FontFace::parse(&bytes).unwrap();
    let loca_format = face.metrics().index_to_loc_format;
    let glyph = find_simple_glyph(&bytes, loca_format, true);
    drop(face);
    let glyf = table_range(&bytes, *b"glyf").0;
    let start = glyph.start;
    bytes[glyf + start + 10..glyf + start + 12].copy_from_slice(&0u16.to_be_bytes());
    bytes[glyf + start + 12..glyf + start + 14].copy_from_slice(&0u16.to_be_bytes());
    bytes[glyf + start + 14] = 0x08;
    bytes[glyf + start + 15] = u8::MAX;
    let face = FontFace::parse(&bytes).unwrap();
    assert!(matches!(
        face.outline(glyph.gid),
        Err(OutlineError::InvalidData("flag repeat count"))
    ));

    let mut bytes = original.data.to_vec();
    let face = FontFace::parse(&bytes).unwrap();
    let glyph = find_simple_glyph(&bytes, face.metrics().index_to_loc_format, true);
    let glyf = table_range(&bytes, *b"glyf").0;
    bytes[glyf + glyph.start + 10..glyf + glyph.start + 12].copy_from_slice(&0u16.to_be_bytes());
    bytes[glyf + glyph.start + 12..glyf + glyph.start + 14].copy_from_slice(&0u16.to_be_bytes());
    bytes[glyf + glyph.start + 14] = 0x01;
    set_loca_offset(
        &mut bytes,
        loca_format,
        glyph.gid as usize + 1,
        glyph.start + 16,
    );
    let face = FontFace::parse(&bytes).unwrap();
    assert_eq!(
        face.outline(glyph.gid).unwrap_err(),
        OutlineError::Truncated
    );
}

#[test]
fn contradictory_composite_transforms_return_typed_errors() {
    let original = carlito_regular();
    let face = FontFace::parse(original.data).unwrap();
    let loca_format = face.metrics().index_to_loc_format;
    let glyph = find_composite_glyph(original.data, loca_format);
    let glyf = table_range(original.data, *b"glyf").0;

    let mut bytes = original.data.to_vec();
    let old = read_u16(&bytes, glyf + glyph.start + 10);
    let flags = (old & 1) | 2 | 0x1800;
    bytes[glyf + glyph.start + 10..glyf + glyph.start + 12].copy_from_slice(&flags.to_be_bytes());
    let face = FontFace::parse(&bytes).unwrap();
    assert_eq!(
        face.outline(glyph.gid).unwrap_err(),
        OutlineError::InvalidData("contradictory component offset flags")
    );

    let mut bytes = original.data.to_vec();
    let old = read_u16(&bytes, glyf + glyph.start + 10);
    let flags = (old & 1) | 2 | 0x88;
    bytes[glyf + glyph.start + 10..glyf + glyph.start + 12].copy_from_slice(&flags.to_be_bytes());
    let face = FontFace::parse(&bytes).unwrap();
    assert_eq!(
        face.outline(glyph.gid).unwrap_err(),
        OutlineError::InvalidData("conflicting component transforms")
    );

    let glyph = find_composite_glyph_min_len(original.data, loca_format, 22);
    let mut bytes = original.data.to_vec();
    bytes[glyf + glyph.start + 10..glyf + glyph.start + 12]
        .copy_from_slice(&(2u16 | 0x80).to_be_bytes());
    set_loca_offset(
        &mut bytes,
        loca_format,
        usize::from(glyph.gid) + 1,
        glyph.start + 20,
    );
    let face = FontFace::parse(&bytes).unwrap();
    assert_eq!(
        face.outline(glyph.gid).unwrap_err(),
        OutlineError::Truncated
    );
}

#[test]
fn nested_composite_budgets_accept_exact_totals_and_reject_one_less() {
    let mut bytes = carlito_regular().data.to_vec();
    let face = FontFace::parse(&bytes).unwrap();
    let loca_format = face.metrics().index_to_loc_format;
    let composites = (0..face.metrics().number_of_glyphs)
        .filter(|&gid| is_composite(&bytes, loca_format, gid))
        .collect::<Vec<_>>();
    drop(face);
    let (gid, range, glyf, nested) = composites
        .iter()
        .find_map(|&root| {
            let range = glyph_range(&bytes, loca_format, usize::from(root));
            let glyf = table_range(&bytes, *b"glyf").0;
            composites
                .iter()
                .copied()
                .find(|&nested| {
                    nested != root
                        && !reaches_glyph(&bytes, loca_format, nested, root, &mut Vec::new())
                })
                .map(|nested| (root, range, glyf, nested))
        })
        .expect("Carlito has composites that can be combined without creating a cycle");
    bytes[glyf + range.start + 12..glyf + range.start + 14].copy_from_slice(&nested.to_be_bytes());
    let maxp = table_range(&bytes, *b"maxp").0;
    bytes[maxp + 10..maxp + 12].copy_from_slice(&u16::MAX.to_be_bytes());
    bytes[maxp + 12..maxp + 14].copy_from_slice(&u16::MAX.to_be_bytes());
    bytes[maxp + 28..maxp + 30].copy_from_slice(&u16::MAX.to_be_bytes());
    bytes[maxp + 30..maxp + 32].copy_from_slice(&u16::MAX.to_be_bytes());
    let face = FontFace::parse(&bytes).unwrap();
    let stats = expanded_stats(&bytes, loca_format, gid, 0);
    assert!(
        stats.max_depth >= 2,
        "mutation should build a nested composite"
    );
    let exact = OutlineLimits {
        max_depth: stats.max_depth,
        max_points: stats.points,
        max_contours: stats.contours,
        max_components: stats.components,
    };
    assert!(
        face.outline_with_limits(gid, exact).is_ok(),
        "exact cumulative budgets should be accepted"
    );
    assert!(matches!(
        face.outline_with_limits(
            gid,
            OutlineLimits {
                max_points: stats.points - 1,
                ..exact
            }
        ),
        Err(OutlineError::LimitExceeded("points"))
    ));
    assert!(matches!(
        face.outline_with_limits(
            gid,
            OutlineLimits {
                max_contours: stats.contours - 1,
                ..exact
            }
        ),
        Err(OutlineError::LimitExceeded("contours"))
    ));
    assert!(matches!(
        face.outline_with_limits(
            gid,
            OutlineLimits {
                max_depth: stats.max_depth - 1,
                ..exact
            }
        ),
        Err(OutlineError::LimitExceeded("composite depth"))
    ));
    assert!(matches!(
        face.outline_with_limits(
            gid,
            OutlineLimits {
                max_components: stats.components - 1,
                ..exact
            }
        ),
        Err(OutlineError::LimitExceeded("components"))
    ));
}

#[derive(Clone, Copy)]
struct GlyphRange {
    gid: u16,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct ExpandedStats {
    points: usize,
    contours: usize,
    components: usize,
    max_depth: usize,
}

fn carlito_regular() -> &'static atelier_font::embedded::EmbeddedFont {
    EMBEDDED_FONTS
        .iter()
        .find(|font| {
            font.family == "Carlito"
                && font.weight == 400
                && font.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap()
}

fn find_simple_glyph(bytes: &[u8], loca_format: i16, need_room: bool) -> GlyphRange {
    (0..glyph_count(bytes))
        .find_map(|gid| {
            let range = glyph_range(bytes, loca_format, gid);
            let contour_count = i16::from_be_bytes(
                bytes[table_range(bytes, *b"glyf").0 + range.start
                    ..table_range(bytes, *b"glyf").0 + range.start + 2]
                    .try_into()
                    .ok()?,
            );
            (contour_count == 1 && (!need_room || range.end - range.start >= 18)).then_some(range)
        })
        .expect("reference font contains a suitable simple glyph")
}

fn find_composite_glyph(bytes: &[u8], loca_format: i16) -> GlyphRange {
    find_composite_glyph_min_len(bytes, loca_format, 14)
}

fn find_composite_glyph_min_len(bytes: &[u8], loca_format: i16, min_len: usize) -> GlyphRange {
    (0..glyph_count(bytes))
        .find_map(|gid| {
            let range = glyph_range(bytes, loca_format, gid);
            let glyf = table_range(bytes, *b"glyf").0;
            (range.end - range.start >= min_len
                && i16::from_be_bytes(
                    bytes[glyf + range.start..glyf + range.start + 2]
                        .try_into()
                        .ok()?,
                ) < 0)
                .then_some(range)
        })
        .expect("reference font contains a composite glyph")
}

fn is_composite(bytes: &[u8], loca_format: i16, gid: u16) -> bool {
    let range = glyph_range(bytes, loca_format, usize::from(gid));
    let glyf = table_range(bytes, *b"glyf").0;
    range.end - range.start >= 14
        && i16::from_be_bytes(
            bytes[glyf + range.start..glyf + range.start + 2]
                .try_into()
                .unwrap(),
        ) < 0
}

fn reaches_glyph(
    bytes: &[u8],
    loca_format: i16,
    from: u16,
    target: u16,
    visited: &mut Vec<u16>,
) -> bool {
    if from == target {
        return true;
    }
    if visited.contains(&from) || !is_composite(bytes, loca_format, from) {
        return false;
    }
    visited.push(from);
    composite_children(bytes, loca_format, from)
        .into_iter()
        .any(|child| reaches_glyph(bytes, loca_format, child, target, visited))
}

fn composite_children(bytes: &[u8], loca_format: i16, gid: u16) -> Vec<u16> {
    let range = glyph_range(bytes, loca_format, usize::from(gid));
    let glyf = table_range(bytes, *b"glyf").0;
    let data = &bytes[glyf + range.start..glyf + range.end];
    let mut cursor = 10;
    let mut children = Vec::new();
    loop {
        let flags = read_u16(data, cursor);
        children.push(read_u16(data, cursor + 2));
        cursor += 4 + if flags & 1 != 0 { 4 } else { 2 };
        cursor += if flags & 0x80 != 0 {
            8
        } else if flags & 0x40 != 0 {
            4
        } else if flags & 8 != 0 {
            2
        } else {
            0
        };
        if flags & 0x20 == 0 {
            return children;
        }
    }
}

fn expanded_stats(bytes: &[u8], loca_format: i16, gid: u16, depth: usize) -> ExpandedStats {
    let range = glyph_range(bytes, loca_format, usize::from(gid));
    let glyf = table_range(bytes, *b"glyf").0;
    let data = &bytes[glyf + range.start..glyf + range.end];
    if data.is_empty() {
        return ExpandedStats {
            max_depth: depth,
            ..ExpandedStats::default()
        };
    }
    let contours = i16::from_be_bytes(data[0..2].try_into().unwrap());
    let mut stats = ExpandedStats {
        max_depth: depth,
        ..ExpandedStats::default()
    };
    if contours >= 0 {
        stats.contours = contours as usize;
        if contours > 0 {
            let endpoint = 10 + (contours as usize - 1) * 2;
            stats.points = usize::from(u16::from_be_bytes(
                data[endpoint..endpoint + 2].try_into().unwrap(),
            )) + 1;
        }
        return stats;
    }
    let mut cursor = 10;
    let mut flags;
    loop {
        flags = read_u16(data, cursor);
        let child = read_u16(data, cursor + 2);
        cursor += 4;
        cursor += if flags & 1 != 0 { 4 } else { 2 };
        cursor += if flags & 0x80 != 0 {
            8
        } else if flags & 0x40 != 0 {
            4
        } else if flags & 8 != 0 {
            2
        } else {
            0
        };
        let child_stats = expanded_stats(bytes, loca_format, child, depth + 1);
        stats.points += child_stats.points;
        stats.contours += child_stats.contours;
        stats.components += child_stats.components + 1;
        stats.max_depth = stats.max_depth.max(child_stats.max_depth);
        if flags & 0x20 == 0 {
            break;
        }
    }
    stats
}

fn glyph_count(bytes: &[u8]) -> usize {
    let maxp = table_range(bytes, *b"maxp").0;
    usize::from(read_u16(bytes, maxp + 4))
}

fn glyph_range(bytes: &[u8], loca_format: i16, gid: usize) -> GlyphRange {
    let loca = table_range(bytes, *b"loca").0;
    let offset = |index| {
        if loca_format == 0 {
            usize::from(read_u16(bytes, loca + index * 2)) * 2
        } else {
            read_u32(bytes, loca + index * 4) as usize
        }
    };
    GlyphRange {
        gid: gid as u16,
        start: offset(gid),
        end: offset(gid + 1),
    }
}

fn set_loca_offset(bytes: &mut [u8], loca_format: i16, index: usize, value: usize) {
    let loca = table_range(bytes, *b"loca").0;
    if loca_format == 0 {
        bytes[loca + index * 2..loca + index * 2 + 2]
            .copy_from_slice(&((value / 2) as u16).to_be_bytes());
    } else {
        bytes[loca + index * 4..loca + index * 4 + 4]
            .copy_from_slice(&(value as u32).to_be_bytes());
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn table_range(bytes: &[u8], tag: [u8; 4]) -> (usize, usize) {
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    for index in 0..count {
        let record = 12 + index * 16;
        if bytes[record..record + 4] == tag {
            let start =
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
            let len =
                u32::from_be_bytes(bytes[record + 12..record + 16].try_into().unwrap()) as usize;
            return (start, len);
        }
    }
    panic!("missing table {:?}", tag);
}
