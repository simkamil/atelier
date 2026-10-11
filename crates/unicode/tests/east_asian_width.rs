// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::properties::east_asian_width;

#[test]
fn every_scalar_matches_official_east_asian_width() {
    let source = include_str!("../../../data/ucd/17.0.0/EastAsianWidth.txt");
    let mut expected = vec!["N"; 0x110000];
    let mut ranges = 0;
    for line in source.lines() {
        let body = line.split('#').next().unwrap().trim();
        if body.is_empty() {
            continue;
        }
        let (range, value) = body.split_once(';').unwrap();
        let mut endpoints = range.trim().split("..");
        let start = usize::from_str_radix(endpoints.next().unwrap(), 16).unwrap();
        let end = endpoints
            .next()
            .map(|s| usize::from_str_radix(s, 16).unwrap())
            .unwrap_or(start);
        expected[start..=end].fill(value.trim());
        ranges += 1;
    }
    assert!(ranges > 2000, "official corpus must be complete");
    let mut scalars = 0;
    for (codepoint, value) in expected.into_iter().enumerate() {
        if let Some(character) = char::from_u32(codepoint as u32) {
            assert_eq!(east_asian_width(character), value, "U+{codepoint:04X}");
            scalars += 1;
        }
    }
    assert_eq!(scalars, 1_112_064);
}
