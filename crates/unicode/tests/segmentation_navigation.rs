// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::segmentation::{
    BoundaryIndex, OffsetError, grapheme_boundaries, word_boundaries,
};

fn verify_navigation(text: &str, expected: &[usize], index: BoundaryIndex<'_>) {
    let ranges: Vec<_> = expected.windows(2).map(|w| w[0]..w[1]).collect();
    assert_eq!(index.segments().collect::<Vec<_>>(), ranges);
    assert_eq!(
        index.segments().rev().collect::<Vec<_>>(),
        ranges.iter().rev().cloned().collect::<Vec<_>>()
    );
    let mut mixed = index.segments();
    let mut first = 0;
    let mut last = ranges.len();
    while first < last {
        assert_eq!(mixed.next(), Some(ranges[first].clone()));
        first += 1;
        if first < last {
            last -= 1;
            assert_eq!(mixed.next_back(), Some(ranges[last].clone()));
        }
    }
    assert_eq!(mixed.next(), None);
    assert_eq!(mixed.next_back(), None);
    for offset in 0..=text.len() {
        if !text.is_char_boundary(offset) {
            let error = OffsetError::NotCharBoundary { offset };
            assert_eq!(index.is_boundary(offset), Err(error));
            assert_eq!(index.floor_boundary(offset), Err(error));
            assert_eq!(index.ceil_boundary(offset), Err(error));
            assert_eq!(index.previous_boundary(offset), Err(error));
            assert_eq!(index.next_boundary(offset), Err(error));
            assert!(matches!(index.forward_from(offset), Err(e) if e == error));
            assert!(matches!(index.backward_from(offset), Err(e) if e == error));
            continue;
        }
        assert_eq!(index.is_boundary(offset), Ok(expected.contains(&offset)));
        assert_eq!(
            index.floor_boundary(offset),
            Ok(*expected.iter().rfind(|&&b| b <= offset).unwrap())
        );
        assert_eq!(
            index.ceil_boundary(offset),
            Ok(*expected.iter().find(|&&b| b >= offset).unwrap())
        );
        assert_eq!(
            index.previous_boundary(offset),
            Ok(expected.iter().copied().rfind(|&b| b < offset))
        );
        assert_eq!(
            index.next_boundary(offset),
            Ok(expected.iter().copied().find(|&b| b > offset))
        );
        assert_eq!(
            index.forward_from(offset).unwrap().collect::<Vec<_>>(),
            ranges
                .iter()
                .filter(|r| r.end > offset)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            index.backward_from(offset).unwrap().collect::<Vec<_>>(),
            ranges
                .iter()
                .rev()
                .filter(|r| r.start < offset)
                .cloned()
                .collect::<Vec<_>>()
        );
    }
    let offset = text.len() + 1;
    let error = OffsetError::OutOfBounds {
        offset,
        len: text.len(),
    };
    assert_eq!(index.is_boundary(offset), Err(error));
    assert_eq!(index.floor_boundary(offset), Err(error));
    assert_eq!(index.ceil_boundary(offset), Err(error));
    assert_eq!(index.previous_boundary(offset), Err(error));
    assert_eq!(index.next_boundary(offset), Err(error));
    assert!(matches!(index.forward_from(offset), Err(e) if e == error));
    assert!(matches!(index.backward_from(offset), Err(e) if e == error));
}

#[test]
fn every_official_case_has_consistent_navigation_at_every_utf8_position() {
    let cases = [
        (
            include_str!("../../../data/ucd/17.0.0/auxiliary/GraphemeBreakTest.txt"),
            false,
        ),
        (
            include_str!("../../../data/ucd/17.0.0/auxiliary/WordBreakTest.txt"),
            true,
        ),
    ];
    let mut count = 0;
    for (source, word) in cases {
        for line in source.lines() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let mut text = String::new();
            let mut expected = Vec::new();
            for token in line.split_whitespace() {
                match token {
                    "÷" => expected.push(text.len()),
                    "×" => {}
                    value => {
                        text.push(char::from_u32(u32::from_str_radix(value, 16).unwrap()).unwrap())
                    }
                }
            }
            let index = if word {
                word_boundaries(&text)
            } else {
                grapheme_boundaries(&text)
            };
            verify_navigation(&text, &expected, index);
            count += 1;
        }
    }
    assert!(count > 2500);
    verify_navigation("", &[0], grapheme_boundaries(""));
    verify_navigation("", &[0], word_boundaries(""));
}
