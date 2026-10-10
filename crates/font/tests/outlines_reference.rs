// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::FontFace;
use atelier_geom::{FlattenedSubpath, Path, Point};
use std::path::Path as FsPath;

const REFERENCE: &str = include_str!("../../../tests/font-outline-reference/outlines.tsv");
const FLATTEN_TOLERANCE: f64 = 0.01;

#[test]
fn embedded_outlines_match_fonttools_reference_geometry() {
    let workspace = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut face_cache = std::collections::BTreeMap::<String, FontFace<'static>>::new();
    let mut font_storage = std::collections::BTreeMap::<String, &'static [u8]>::new();
    let mut cases = 0;

    for (line_number, line) in REFERENCE.lines().enumerate() {
        let mut fields = line.splitn(3, '\t');
        let font_path = fields.next().expect("font path field");
        let glyph_id: u16 = fields
            .next()
            .expect("glyph id field")
            .parse()
            .expect("valid glyph id");
        let commands = fields.next().expect("outline field");
        if !face_cache.contains_key(font_path) {
            let absolute = workspace.join(font_path);
            let bytes: &'static [u8] = Box::leak(
                std::fs::read(&absolute)
                    .unwrap_or_else(|e| panic!("{}: {e}", absolute.display()))
                    .into_boxed_slice(),
            );
            let face =
                FontFace::parse(bytes).unwrap_or_else(|e| panic!("{}: {e}", absolute.display()));
            font_storage.insert(font_path.to_owned(), bytes);
            face_cache.insert(font_path.to_owned(), face);
        }
        let expected = parse_reference_path(commands)
            .unwrap_or_else(|e| panic!("line {}: {e}", line_number + 1));
        let expected = expected
            .flatten(FLATTEN_TOLERANCE)
            .expect("reference path flattens");
        let actual_path = face_cache[font_path]
            .outline(glyph_id)
            .unwrap_or_else(|e| panic!("{} glyph {glyph_id}: {e}", font_path));
        let actual = actual_path
            .flatten(FLATTEN_TOLERANCE)
            .unwrap_or_else(|e| panic!("{} glyph {glyph_id}: {e}", font_path));
        assert_same_contours(&expected, &actual, font_path, glyph_id);
        cases += 1;
    }

    assert_eq!(
        cases, 300,
        "the reference corpus should contain all 300 FontTools cases"
    );
    assert_eq!(
        font_storage.len(),
        20,
        "the reference corpus should cover all 20 embedded faces"
    );
}

fn parse_reference_path(commands: &str) -> Result<Path, String> {
    let mut path = Path::new();
    let mut words = commands.split_whitespace();
    while let Some(command) = words.next() {
        match command {
            "M" => path.move_to(read_point(&mut words)?),
            "L" => path
                .line_to(read_point(&mut words)?)
                .map_err(|e| format!("line: {e:?}"))?,
            "Q" => {
                let control = read_point(&mut words)?;
                let end = read_point(&mut words)?;
                path.quadratic_to(control, end)
                    .map_err(|e| format!("quadratic: {e:?}"))?;
            }
            "Z" => path.close().map_err(|e| format!("close: {e:?}"))?,
            other => return Err(format!("unknown path command {other:?}")),
        }
    }
    Ok(path)
}

fn read_point<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Point, String> {
    let x: f64 = words
        .next()
        .ok_or("missing x coordinate")?
        .parse()
        .map_err(|_| "invalid x coordinate")?;
    let y: f64 = words
        .next()
        .ok_or("missing y coordinate")?
        .parse()
        .map_err(|_| "invalid y coordinate")?;
    Point::try_new(x, y).map_err(|e| format!("invalid point: {e:?}"))
}

fn assert_same_contours(
    expected: &[FlattenedSubpath],
    actual: &[FlattenedSubpath],
    font: &str,
    glyph: u16,
) {
    assert_eq!(
        expected.len(),
        actual.len(),
        "{font} glyph {glyph}: contour count differs"
    );
    let expected = expected.iter().map(normalized_points).collect::<Vec<_>>();
    let actual = actual.iter().map(normalized_points).collect::<Vec<_>>();
    let edges = expected
        .iter()
        .map(|e| {
            actual
                .iter()
                .map(|a| same_closed_contour(e, a))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut assigned = vec![None; actual.len()];
    for expected_index in 0..expected.len() {
        let mut seen = vec![false; actual.len()];
        assert!(
            assign_contour(expected_index, &edges, &mut assigned, &mut seen),
            "{font} glyph {glyph}: flattened contour geometry differs"
        );
    }
}

fn normalized_points(contour: &FlattenedSubpath) -> Vec<Point> {
    assert!(contour.closed, "TrueType reference contours must be closed");
    let mut points = contour.points.clone();
    // Path::close records closure separately. The outline decoder also emits
    // the explicit closing edge, so remove its duplicated endpoint here.
    if points.len() > 1 && close_enough(*points.first().unwrap(), *points.last().unwrap()) {
        points.pop();
    }
    points
}

fn same_closed_contour(expected: &[Point], actual: &[Point]) -> bool {
    if expected.len() != actual.len() || expected.is_empty() {
        return expected.is_empty() && actual.is_empty();
    }
    (0..actual.len()).any(|rotation| {
        expected
            .iter()
            .enumerate()
            .all(|(index, point)| close_enough(*point, actual[(index + rotation) % actual.len()]))
    })
}

fn close_enough(a: Point, b: Point) -> bool {
    (a.x() - b.x()).hypot(a.y() - b.y()) <= FLATTEN_TOLERANCE
}

fn assign_contour(
    index: usize,
    edges: &[Vec<bool>],
    assigned: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for actual in 0..assigned.len() {
        if !edges[index][actual] || seen[actual] {
            continue;
        }
        seen[actual] = true;
        if assigned[actual].is_none()
            || assign_contour(assigned[actual].unwrap(), edges, assigned, seen)
        {
            assigned[actual] = Some(index);
            return true;
        }
    }
    false
}
