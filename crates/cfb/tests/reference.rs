// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reference files created and read back through Microsoft's Structured Storage COM API.

use atelier_cfb::{Compound, EntryKind, Limits};

fn pattern(tag: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 31 + usize::from(tag) * 17) % 251) as u8)
        .collect()
}

fn check_reference(bytes: &[u8], version: u16) {
    let c = Compound::open(bytes, Limits::default()).expect("native CFB fixture opens");
    assert_eq!(c.major_version(), version);
    assert!(c.is_storage("Unicode"));
    assert!(c.is_storage("Unicode/Δοκιμή"));
    let expected = [
        ("empty", vec![]),
        ("mini", pattern(1, 300)),
        ("boundary4095", pattern(2, 4095)),
        ("boundary4096", pattern(3, 4096)),
        ("multi8193", pattern(4, 8193)),
        ("Unicode/Δοκιμή/東京", pattern(5, 73)),
    ];
    for (path, contents) in expected {
        let entry = c.find(path).unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(entry.kind(), EntryKind::Stream, "{path}");
        assert_eq!(c.read(path).unwrap(), contents, "{path}");
    }
}

#[test]
fn microsoft_structured_storage_v3_reference() {
    check_reference(
        include_bytes!("../../../tests/cfb-reference/native-v3.cfb"),
        3,
    );
}

#[test]
fn microsoft_structured_storage_v4_reference() {
    check_reference(
        include_bytes!("../../../tests/cfb-reference/native-v4.cfb"),
        4,
    );
}
