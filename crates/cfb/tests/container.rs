// SPDX-License-Identifier: AGPL-3.0-or-later
mod common;

use atelier_cfb::{Anomaly, CfbError, Compound, EntryKind, Limits, is_cfb};
use common::*;

const CLSID: [u8; 16] = [
    0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 1, 2, 3, 4, 5, 6, 7, 8,
];

fn nodes() -> Vec<Node> {
    vec![
        stream("Small", &pattern(1, 100)),
        stream("Empty", b""),
        stream("Mini4095", &pattern(2, 4095)),
        stream("Big4096", &pattern(3, 4096)),
        stream("Big", &pattern(4, 20_000)),
        storage_clsid(
            "Dir",
            CLSID,
            vec![
                stream("Inner", &pattern(5, 700)),
                storage("Sub", vec![stream("Deep", &pattern(6, 5000))]),
            ],
        ),
    ]
}

fn expected() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Small", pattern(1, 100)),
        ("Empty", vec![]),
        ("Mini4095", pattern(2, 4095)),
        ("Big4096", pattern(3, 4096)),
        ("Big", pattern(4, 20_000)),
        ("Dir/Inner", pattern(5, 700)),
        ("Dir/Sub/Deep", pattern(6, 5000)),
    ]
}

fn sample(version: u16) -> Built {
    Builder::new(version, nodes()).build()
}

fn open(b: &Built) -> Compound<'_> {
    Compound::open(&b.bytes, Limits::default()).unwrap()
}

fn open_err(bytes: &[u8]) -> CfbError {
    match Compound::open(bytes, Limits::default()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

fn assert_all_readable(c: &Compound<'_>, skip: &[&str]) {
    for (p, data) in expected() {
        if !skip.contains(&p) {
            assert_eq!(c.read(p).unwrap(), data, "{p}");
        }
    }
}

// ---------------------------------------------------------------- valid files

#[test]
fn reads_every_stream_in_both_versions() {
    for v in [3, 4] {
        let b = sample(v);
        let c = open(&b);
        assert_eq!(c.major_version(), v);
        assert_eq!(c.sector_size(), if v == 3 { 512 } else { 4096 });
        assert!(c.anomalies().is_empty(), "{:?}", c.anomalies());
        assert_all_readable(&c, &[]);
    }
}

#[test]
fn list_is_sorted_and_depth_first() {
    for v in [3, 4] {
        let b = sample(v);
        let c = open(&b);
        let paths: Vec<_> = c.list().iter().map(|l| l.path.clone()).collect();
        assert_eq!(
            paths,
            [
                "Big",
                "Dir",
                "Dir/Sub",
                "Dir/Sub/Deep",
                "Dir/Inner",
                "Empty",
                "Small",
                "Big4096",
                "Mini4095"
            ]
        );
        let l = c.list();
        assert_eq!(l[3].depth, 3);
        assert_eq!(l[3].entry.kind(), EntryKind::Stream);
        assert_eq!(l[1].entry.kind(), EntryKind::Storage);
    }
}

#[test]
fn existence_kinds_and_clsid() {
    let mut b = Builder::new(3, nodes());
    b.root_clsid = [9; 16];
    let b = b.build();
    let c = open(&b);
    assert_eq!(c.root_clsid(), [9; 16]);
    assert_eq!(c.root().kind(), EntryKind::Root);
    assert_eq!(c.find("Dir").unwrap().clsid(), CLSID);
    assert!(c.is_storage("Dir") && !c.is_stream("Dir"));
    assert!(c.is_storage("") && c.is_storage("/"));
    assert!(c.is_stream("/Dir/Inner") && c.exists("Dir//Sub"));
    assert!(!c.exists("Nope") && !c.exists("Small/x") && !c.exists("Dir/Nope"));
    assert_eq!(c.read("Dir"), Err(CfbError::NotAStream));
    assert_eq!(c.read("Nope"), Err(CfbError::NotFound));
    assert_eq!(c.find("Big").unwrap().size(), 20_000);
    assert_eq!(c.children(c.find("Dir").unwrap()).count(), 2);
}

#[test]
fn entries_cannot_be_used_with_another_compound() {
    let first = Builder::new(3, vec![storage("Folder", vec![stream("Same", b"first")])]).build();
    let second = Builder::new(3, vec![storage("Folder", vec![stream("Same", b"second")])]).build();
    let a = Compound::open(&first.bytes, Limits::default()).unwrap();
    let b = Compound::open(&second.bytes, Limits::default()).unwrap();
    let foreign_stream = a.find("Folder/Same").unwrap();
    let foreign_storage = a.find("Folder").unwrap();
    assert_eq!(foreign_stream.id(), b.find("Folder/Same").unwrap().id());
    assert_eq!(b.read("Folder/Same").unwrap(), b"second");
    assert_eq!(
        b.read_entry_range(foreign_stream, 0, 5),
        Err(CfbError::ForeignEntry)
    );
    assert_eq!(b.children(foreign_storage).count(), 0);
}

#[test]
fn fat_and_mini_fat_selection_at_the_cutoff() {
    for v in [3, 4] {
        let b = sample(v);
        let c = open(&b);
        // 4095 bytes live in the mini stream, 4096 in regular sectors.
        assert_eq!(c.find("Mini4095").unwrap().size(), 4095);
        assert!(b.start_of("Mini4095") < 64 + 1000);
        assert_eq!(c.read("Mini4095").unwrap(), pattern(2, 4095));
        assert_eq!(c.read("Big4096").unwrap(), pattern(3, 4096));
    }
}

#[test]
fn empty_container_has_only_a_root() {
    for v in [3, 4] {
        let b = Builder::new(v, vec![]).build();
        let c = open(&b);
        assert!(c.list().is_empty());
        assert!(c.anomalies().is_empty());
    }
}

#[test]
fn large_stream_needs_many_fat_sectors_and_difat() {
    // 7.3 MB at 512 bytes per sector needs more than 109 FAT sectors.
    let data = pattern(9, 7_300_000);
    let b = Builder::new(3, vec![stream("Huge", &data), stream("s", b"x")]).build();
    assert!(b.fat_sectors.len() > 109 && !b.difat_sectors.is_empty());
    let c = open(&b);
    assert_eq!(c.read("Huge").unwrap(), data);
    assert_eq!(c.read("s").unwrap(), b"x");
    assert_eq!(
        c.read_range("Huge", 7_299_990, 100).unwrap(),
        data[7_299_990..]
    );
}

#[test]
fn difat_chains_in_both_versions() {
    for (v, fats, difats) in [(3, 300, 2), (4, 111, 1), (3, 110, 1)] {
        let mut bd = Builder::new(v, nodes());
        bd.min_fat_sectors = fats;
        let b = bd.build();
        assert_eq!(b.difat_sectors.len(), difats);
        let c = open(&b);
        assert_all_readable(&c, &[]);
    }
}

#[test]
fn deep_tree_and_depth_limit() {
    fn nest(n: usize) -> Node {
        if n == 0 {
            stream("leaf", b"deep")
        } else {
            storage(&format!("d{n}"), vec![nest(n - 1), stream("side", b"s")])
        }
    }
    let b = Builder::new(3, vec![nest(40), stream("top", b"t")]).build();
    let c = open(&b);
    let mut path = String::new();
    for n in (1..=40).rev() {
        path.push_str(&format!("d{n}/"));
    }
    assert_eq!(c.read(&format!("{path}leaf")).unwrap(), b"deep");
    let limits = Limits {
        max_depth: 10,
        ..Limits::default()
    };
    let c = Compound::open(&b.bytes, limits).unwrap();
    assert!(
        c.anomalies()
            .iter()
            .any(|a| matches!(a, Anomaly::DepthExceeded(_)))
    );
    assert!(!c.exists(&format!("{path}leaf")));
    assert_eq!(c.read("top").unwrap(), b"t");
    assert_eq!(c.read("d40/d39/side").unwrap(), b"s");
}

#[test]
fn names_are_case_insensitive_on_utf16_units() {
    let b = Builder::new(
        3,
        vec![
            stream("Éclair", b"1"),
            stream("σίγμα", b"2"),
            stream("Привет", b"3"),
            stream("\u{1d11e}clef", b"4"),
            stream("\u{1}CompObj", b"5"),
            stream("ünï.txt", b"6"),
        ],
    )
    .build();
    let c = open(&b);
    assert_eq!(c.read("éCLAIR").unwrap(), b"1");
    assert_eq!(c.read("ΣΊΓΜΑ").unwrap(), b"2");
    assert_eq!(c.read("привет").unwrap(), b"3");
    assert_eq!(c.read("\u{1d11e}CLEF").unwrap(), b"4");
    assert_eq!(c.read("\u{1}compobj").unwrap(), b"5");
    assert_eq!(c.read("ÜNÏ.TXT").unwrap(), b"6");
    assert!(!c.exists("Eclair"));
    assert!(c.anomalies().is_empty());
}

#[test]
fn degenerate_unsorted_tree_is_not_trusted() {
    let many: Vec<Node> = (0..60)
        .map(|i| stream(&format!("n{:02}", (i * 7) % 60), &pattern(i, 10)))
        .collect();
    let mut bd = Builder::new(3, many);
    bd.degenerate_tree = true;
    let b = bd.build();
    let c = open(&b);
    assert!(c.anomalies().is_empty());
    let paths: Vec<_> = c.list().iter().map(|l| l.path.clone()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
    assert_eq!(paths.len(), 60);
    assert_eq!(c.read("N07").unwrap(), pattern(1, 10));
}

#[test]
fn partial_reads() {
    for v in [3, 4] {
        let b = sample(v);
        let c = open(&b);
        for (p, data) in expected() {
            let n = data.len();
            for (off, len) in [
                (0, 10),
                (n / 2, 100),
                (n.saturating_sub(5), 100),
                (n, 5),
                (3, 0),
            ] {
                if off > n {
                    continue;
                }
                let end = (off + len).min(n);
                assert_eq!(
                    c.read_range(p, off as u64, len).unwrap(),
                    data[off..end],
                    "{p} {off}+{len}"
                );
            }
            assert_eq!(c.read_range(p, n as u64 + 1, 1), Err(CfbError::OutOfRange));
        }
        // Crossing sector and mini-sector boundaries.
        let big = pattern(4, 20_000);
        assert_eq!(c.read_range("Big", 500, 600).unwrap(), big[500..1100]);
        assert_eq!(c.read_range("Big", 4000, 9000).unwrap(), big[4000..13000]);
        let m = pattern(2, 4095);
        assert_eq!(c.read_range("Mini4095", 60, 10).unwrap(), m[60..70]);
        assert_eq!(c.read_range("Big", 0, usize::MAX).unwrap(), big);
    }
}

#[test]
fn detection_distinguishes_cfb_from_zip() {
    let b = sample(3);
    assert!(is_cfb(&b.bytes));
    assert!(!is_cfb(b"PK\x03\x04rest"));
    assert!(!is_cfb(&[]));
    assert!(!is_cfb(&b.bytes[..7]));
    assert!(is_cfb(&b.bytes[..8]));
}

// ----------------------------------------------------------------- header

type Case = (usize, &'static [u8], fn(&CfbError) -> bool);

#[test]
fn header_corruptions() {
    let b = sample(3);
    assert_eq!(open_err(b"PK\x03\x04 not cfb at all"), CfbError::NotCfb);
    assert_eq!(open_err(&[]), CfbError::NotCfb);
    assert_eq!(open_err(&b.bytes[..100]), CfbError::Truncated);
    assert_eq!(open_err(&b.bytes[..511]), CfbError::Truncated);
    let cases: [Case; 6] = [
        (26, &[5, 0], |e| {
            matches!(e, CfbError::UnsupportedVersion(5))
        }),
        (28, &[0xff, 0xfe], |e| {
            matches!(e, CfbError::InvalidHeader(_))
        }),
        (30, &[12, 0], |e| matches!(e, CfbError::InvalidHeader(_))),
        (32, &[7, 0], |e| matches!(e, CfbError::InvalidHeader(_))),
        (56, &[0, 0x20, 0, 0], |e| {
            matches!(e, CfbError::InvalidHeader(_))
        }),
        (26, &[4, 0], |e| matches!(e, CfbError::InvalidHeader(_))),
    ];
    for (o, v, check) in cases {
        let mut x = b.bytes.clone();
        x[o..o + v.len()].copy_from_slice(v);
        let e = open_err(&x);
        assert!(check(&e), "{o}: {e:?}");
    }
    // Version 4 needs a 4096-byte header sector.
    let b4 = sample(4);
    assert_eq!(open_err(&b4.bytes[..2000]), CfbError::Truncated);
}

#[test]
fn soft_header_deviations_are_anomalies() {
    let mut b = sample(3);
    b.bytes[10] = 1;
    b.bytes[24] = 0x10;
    b.bytes[36] = 7;
    let c = open(&b);
    assert_eq!(
        c.anomalies()
            .iter()
            .filter(|a| matches!(a, Anomaly::HeaderField(_)))
            .count(),
        3
    );
    assert_all_readable(&c, &[]);
}

#[test]
fn truncated_files_fail_cleanly() {
    for v in [3, 4] {
        let b = sample(v);
        let first_fat = b.sector_off(b.fat_sectors[0]);
        assert!(Compound::open(&b.bytes[..first_fat], Limits::default()).is_err());
        assert!(Compound::open(&b.bytes[..b.bytes.len() - 1], Limits::default()).is_err());
        let dir = b.sector_off(b.dir_start);
        assert!(Compound::open(&b.bytes[..dir], Limits::default()).is_err());
        for cut in (0..b.bytes.len()).step_by(97) {
            let _ = Compound::open(&b.bytes[..cut], Limits::default());
        }
    }
}

#[test]
fn trailing_partial_sector_is_flagged_and_data_stays_readable() {
    let mut b = sample(3);
    b.bytes.extend_from_slice(&[1, 2, 3]);
    let c = open(&b);
    assert!(
        c.anomalies()
            .contains(&Anomaly::HeaderField("partial trailing sector"))
    );
    assert_all_readable(&c, &[]);
}

// ------------------------------------------------------- FAT and DIFAT

#[test]
fn fat_count_corruptions() {
    let b = sample(3);
    let mut x = b.clone_bytes();
    put_u32(&mut x, 44, 0);
    assert!(matches!(open_err(&x), CfbError::InvalidFat(_)));
    let mut x = b.clone_bytes();
    put_u32(&mut x, 44, 100_000);
    assert!(matches!(open_err(&x), CfbError::InvalidFat(_)));
    let mut x = b.clone_bytes();
    put_u32(&mut x, 72, 3);
    assert!(matches!(open_err(&x), CfbError::InvalidDifat(_)));
}

#[test]
fn unused_header_difat_slots_must_be_free() {
    let mut b = sample(3);
    let nfat = get_u32(&b.bytes, 44) as usize;
    assert!(nfat < 109);
    put_u32(&mut b.bytes, 76 + 4 * nfat, ENDOFCHAIN);
    assert!(matches!(
        open_err(&b.bytes),
        CfbError::InvalidDifat("unused header DIFAT entry")
    ));
}

#[test]
fn unused_extended_difat_slots_must_be_free() {
    let mut bd = Builder::new(3, nodes());
    bd.min_fat_sectors = 300;
    let mut b = bd.build();
    let remaining = (b.fat_sectors.len() - 109) % 127;
    assert_ne!(remaining, 0);
    let final_difat = *b.difat_sectors.last().unwrap();
    let unused_slot = b.sector_off(final_difat) + 4 * remaining;
    put_u32(&mut b.bytes, unused_slot, ENDOFCHAIN);
    assert!(matches!(
        open_err(&b.bytes),
        CfbError::InvalidDifat("unused DIFAT entry")
    ));
}

#[test]
fn fat_padding_past_physical_eof_must_be_free() {
    let mut b = sample(3);
    let first_padding = b.sector_count as usize;
    assert!(first_padding < b.fat_sectors.len() * (b.ss / 4));
    b.set_fat(first_padding as u32, ENDOFCHAIN);
    assert!(matches!(
        open_err(&b.bytes),
        CfbError::InvalidFat("non-free FAT entry past end of file")
    ));
}

#[test]
fn allocation_sector_markers_are_validated() {
    let mut b = sample(3);
    let sector = b.fat_sectors[0];
    b.set_fat(sector, ENDOFCHAIN);
    assert_eq!(
        open_err(&b.bytes),
        CfbError::InvalidFatSectorMarker {
            sector,
            marker: ENDOFCHAIN,
        }
    );

    let mut bd = Builder::new(3, nodes());
    bd.min_fat_sectors = 300;
    let mut b = bd.build();
    let sector = b.difat_sectors[0];
    b.set_fat(sector, ENDOFCHAIN);
    assert_eq!(
        open_err(&b.bytes),
        CfbError::InvalidDifatSectorMarker {
            sector,
            marker: ENDOFCHAIN,
        }
    );
}

#[test]
fn too_short_fat_cannot_mark_a_listed_fat_sector() {
    let mut b = Builder::new(3, vec![stream("Large", &pattern(9, 80_000))]).build();
    let sector = b.fat_sectors[0];
    assert!(sector >= 128);
    put_u32(&mut b.bytes, 44, 1);
    put_u32(&mut b.bytes, 76 + 4, FREESECT);
    assert_eq!(open_err(&b.bytes), CfbError::FatTooShort { sector });
}

#[test]
fn header_difat_entries_are_validated() {
    let mut bd = Builder::new(3, nodes());
    bd.min_fat_sectors = 3;
    let b = bd.build();
    let mut x = b.clone_bytes();
    put_u32(&mut x, 76 + 4, FREESECT);
    assert!(matches!(open_err(&x), CfbError::InvalidSector(FREESECT)));
    for special in [ENDOFCHAIN, FATSECT, DIFSECT, b.sector_count + 5] {
        let mut x = b.clone_bytes();
        put_u32(&mut x, 76 + 8, special);
        assert!(matches!(open_err(&x), CfbError::InvalidSector(s) if s == special));
    }
    let mut x = b.clone_bytes();
    let first = get_u32(&x, 76);
    put_u32(&mut x, 76 + 4, first);
    assert!(matches!(open_err(&x), CfbError::InvalidFat(_)));
}

#[test]
fn difat_chain_corruptions() {
    let mut bd = Builder::new(3, nodes());
    bd.min_fat_sectors = 300;
    let b = bd.build();
    let last_slot = |s: u32| b.sector_off(s) + 4 * 127;
    // Cycle: the second DIFAT sector points back at the first.
    let mut x = b.clone_bytes();
    put_u32(&mut x, last_slot(b.difat_sectors[1]), b.difat_sectors[0]);
    assert!(matches!(
        open_err(&x),
        CfbError::ChainCycle | CfbError::InvalidDifat(_)
    ));
    // Self loop.
    let mut x = b.clone_bytes();
    put_u32(&mut x, last_slot(b.difat_sectors[0]), b.difat_sectors[0]);
    assert_eq!(open_err(&x), CfbError::ChainCycle);
    // Chain that does not end.
    let mut x = b.clone_bytes();
    put_u32(&mut x, last_slot(b.difat_sectors[1]), 0);
    assert!(matches!(open_err(&x), CfbError::InvalidDifat(_)));
    // Pointing outside the file, and FREESECT instead of the next sector.
    for bad in [b.sector_count + 9, FREESECT, DIFSECT] {
        let mut x = b.clone_bytes();
        put_u32(&mut x, last_slot(b.difat_sectors[0]), bad);
        assert!(matches!(open_err(&x), CfbError::InvalidSector(s) if s == bad));
    }
    // First DIFAT sector outside the file.
    let mut x = b.clone_bytes();
    put_u32(&mut x, 68, 0xffff_0000);
    assert!(matches!(open_err(&x), CfbError::InvalidSector(_)));
    // Declared count too small.
    let mut x = b.clone_bytes();
    put_u32(&mut x, 72, 1);
    assert!(matches!(open_err(&x), CfbError::InvalidDifat(_)));
}

#[test]
fn fat_sector_listed_in_difat_twice() {
    let mut bd = Builder::new(3, nodes());
    bd.min_fat_sectors = 300;
    let b = bd.build();
    let mut x = b.clone_bytes();
    let dup = get_u32(&x, b.sector_off(b.difat_sectors[0]));
    put_u32(&mut x, b.sector_off(b.difat_sectors[0]) + 4, dup);
    assert!(matches!(open_err(&x), CfbError::InvalidFat(_)));
}

#[test]
fn cycle_in_fat_chain() {
    for v in [3, 4] {
        let mut b = sample(v);
        let s = b.start_of("Big");
        b.set_fat(s + 1, s);
        let c = open(&b);
        assert_eq!(c.read("Big"), Err(CfbError::ChainCycle));
        assert_all_readable(&c, &["Big"]);
        let mut b = sample(v);
        b.set_fat(s, s);
        assert_eq!(open(&b).read("Big"), Err(CfbError::ChainCycle));
    }
}

#[test]
fn chain_leaves_the_file() {
    let mut b = sample(3);
    let s = b.start_of("Big");
    let outside = b.sector_count + 10;
    b.set_fat(s + 2, outside);
    let c = open(&b);
    assert_eq!(c.read("Big"), Err(CfbError::InvalidSector(outside)));
    assert_all_readable(&c, &["Big"]);
}

#[test]
fn special_values_misplaced_in_a_chain() {
    for special in [FREESECT, FATSECT, DIFSECT, 0xffff_fffb, 0xffff_fffa] {
        let mut b = sample(3);
        let s = b.start_of("Big");
        b.set_fat(s + 1, special);
        let c = open(&b);
        assert_eq!(c.read("Big"), Err(CfbError::InvalidSector(special)));
        assert_all_readable(&c, &["Big"]);
    }
    // A stream whose first sector is a FAT sector.
    let mut b = sample(3);
    let fat0 = b.fat_sectors[0];
    b.set_entry_u32("Big", 116, fat0);
    let c = open(&b);
    assert!(c.read("Big").is_err());
    // Start sector with special values.
    for special in [FREESECT, FATSECT, DIFSECT] {
        let mut b = sample(3);
        b.set_entry_u32("Big", 116, special);
        assert_eq!(open(&b).read("Big"), Err(CfbError::InvalidSector(special)));
    }
}

#[test]
fn chain_shorter_than_declared_size() {
    for v in [3, 4] {
        let mut b = sample(v);
        let s = b.start_of("Big");
        b.set_fat(s + 1, ENDOFCHAIN);
        let c = open(&b);
        assert_eq!(c.read("Big"), Err(CfbError::ChainTooShort));
        assert_eq!(c.read_range("Big", 0, 1), Err(CfbError::ChainTooShort));
        assert_all_readable(&c, &["Big"]);
        let mut b = sample(v);
        b.set_entry_u32("Big", 116, ENDOFCHAIN);
        assert_eq!(open(&b).read("Big"), Err(CfbError::ChainTooShort));
    }
}

#[test]
fn chain_longer_than_declared_size() {
    let mut b = sample(3);
    let s = b.start_of("Big");
    let last = s + 19_999 / 512;
    let deep = b.start_of("Dir/Sub/Deep");
    b.set_fat(last, deep);
    let c = open(&b);
    assert_eq!(c.read("Big"), Err(CfbError::ChainTooLong));
    assert_all_readable(&c, &["Big"]);
}

#[test]
fn absurd_declared_sizes() {
    for v in [3, 4] {
        let mut b = sample(v);
        let o = b.entry_off(b.id_of("Big")) + 120;
        b.set_u64(o, 1 << 40);
        let c = open(&b);
        // v3 keeps only the low 32 bits (zero), v4 hits the stream limit.
        if v == 4 {
            let e = c.read("Big").unwrap_err();
            assert!(matches!(e, CfbError::LimitExceeded { .. }));
        } else {
            assert_eq!(c.read("Big").unwrap(), b"");
        }
        // Within the limit but far larger than the chain: no huge allocation.
        b.set_u64(o, 200 * 1024 * 1024);
        assert_eq!(open(&b).read("Big"), Err(CfbError::ChainTooShort));
        b.set_u64(o, 0xffff_ffff_ffff_ffff);
        let _ = open(&b).read("Big");
    }
}

#[test]
fn v3_ignores_the_high_size_bits() {
    let mut b = sample(3);
    let o = b.entry_off(b.id_of("Big")) + 124;
    b.set_u32(o, 0xdead_beef);
    assert_eq!(open(&b).read("Big").unwrap(), pattern(4, 20_000));
}

#[test]
fn v3_stream_above_two_gib_is_flagged() {
    let mut b = sample(3);
    b.set_entry_u32("Big", 120, 0x9000_0000);
    let c = open(&b);
    assert!(
        c.anomalies()
            .iter()
            .any(|a| matches!(a, Anomaly::OversizedV3Stream(_)))
    );
    assert!(matches!(c.read("Big"), Err(CfbError::LimitExceeded { .. })));
}

// ------------------------------------------------------------ mini FAT

fn mini_slot(b: &Built, i: u32) -> usize {
    b.sector_off(b.minifat_start) + 4 * i as usize
}

#[test]
fn mini_fat_cycle_and_bad_references() {
    let mut b = sample(3);
    let s = b.start_of("Mini4095");
    let o = mini_slot(&b, s + 3);
    b.set_u32(o, s);
    let c = open(&b);
    assert_eq!(c.read("Mini4095"), Err(CfbError::ChainCycle));
    assert_all_readable(&c, &["Mini4095"]);

    let mut b = sample(3);
    let s = b.start_of("Mini4095");
    let o = mini_slot(&b, s + 3);
    b.set_u32(o, 99_999);
    assert_eq!(
        open(&b).read("Mini4095"),
        Err(CfbError::InvalidSector(99_999))
    );

    for special in [FREESECT, FATSECT, DIFSECT] {
        let mut b = sample(3);
        let s = b.start_of("Mini4095");
        let o = mini_slot(&b, s + 1);
        b.set_u32(o, special);
        assert_eq!(
            open(&b).read("Mini4095"),
            Err(CfbError::InvalidSector(special))
        );
    }
    let mut b = sample(3);
    let s = b.start_of("Mini4095");
    let o = mini_slot(&b, s + 5);
    b.set_u32(o, ENDOFCHAIN);
    assert_eq!(open(&b).read("Mini4095"), Err(CfbError::ChainTooShort));
}

#[test]
fn mini_chain_longer_than_declared_size() {
    let mut b = sample(3);
    let s = b.start_of("Small");
    let o = mini_slot(&b, s + 1);
    b.set_u32(o, b.start_of("Dir/Inner"));
    assert_eq!(open(&b).read("Small"), Err(CfbError::ChainTooLong));
}

#[test]
fn damaged_mini_fat_keeps_regular_streams_readable() {
    let mini = ["Small", "Mini4095", "Dir/Inner"];
    // Wrong mini FAT sector count, bad start, cycle in the mini FAT chain.
    let mut cases: Vec<Built> = Vec::new();
    let mut b = sample(3);
    b.set_u32(64, 9);
    cases.push(b);
    let mut b = sample(3);
    b.set_u32(60, 0xffff_fff0);
    cases.push(b);
    let mut b = sample(3);
    let m = b.minifat_start;
    b.set_fat(m, m);
    cases.push(b);
    // Mini stream chain cut short / absurd size / leaving the file.
    let mut b = sample(3);
    let o = b.entry_off(0);
    b.set_u32(o + 116, ENDOFCHAIN);
    cases.push(b);
    let mut b = sample(3);
    let o = b.entry_off(0);
    b.set_u64(o + 120, 1 << 40);
    cases.push(b);
    let mut b = sample(3);
    let ms = b.ministream_start;
    b.set_fat(ms, b.sector_count + 3);
    cases.push(b);
    for b in &cases {
        let c = open(b);
        for p in mini {
            assert!(c.read(p).is_err(), "{p}");
        }
        assert_eq!(c.read("Big").unwrap(), pattern(4, 20_000));
        assert_eq!(c.read("Dir/Sub/Deep").unwrap(), pattern(6, 5000));
        assert_eq!(c.read("Empty").unwrap(), b"");
    }
}

// ------------------------------------------------------------ directory

#[test]
fn directory_chain_problems() {
    let b = sample(3);
    assert!(b.dir_sectors >= 2);
    let mut x = sample(3);
    x.set_fat(b.dir_start + 1, b.dir_start);
    assert_eq!(open_err(&x.bytes), CfbError::ChainCycle);
    let mut x = sample(3);
    x.set_u32(48, ENDOFCHAIN);
    assert!(matches!(open_err(&x.bytes), CfbError::InvalidDirectory(_)));
    let mut x = sample(3);
    x.set_u32(48, b.sector_count + 1);
    assert!(matches!(open_err(&x.bytes), CfbError::InvalidSector(_)));
    let mut x = sample(3);
    x.set_u32(48, FREESECT);
    assert!(matches!(open_err(&x.bytes), CfbError::InvalidSector(_)));
    // Directory chain that ends in a special value.
    let mut x = sample(3);
    x.set_fat(b.dir_start + 1, FATSECT);
    assert!(matches!(open_err(&x.bytes), CfbError::InvalidSector(_)));
}

#[test]
fn root_entry_must_be_a_root() {
    let mut b = sample(3);
    let o = b.entry_off(0);
    b.bytes[o + 66] = 2;
    assert!(matches!(open_err(&b.bytes), CfbError::InvalidDirectory(_)));
    let mut b = sample(3);
    let o = b.entry_off(0);
    b.set_u16(o + 64, 0);
    assert!(matches!(open_err(&b.bytes), CfbError::InvalidDirectory(_)));
}

#[test]
fn entry_limit_is_checked_before_loading() {
    let b = sample(3);
    let limits = Limits {
        max_entries: 4,
        ..Limits::default()
    };
    assert!(matches!(
        Compound::open(&b.bytes, limits),
        Err(CfbError::LimitExceeded { .. })
    ));
}

fn find_anomaly(c: &Compound<'_>, f: impl Fn(&Anomaly) -> bool) -> bool {
    c.anomalies().iter().any(f)
}

#[test]
fn cycle_in_the_sibling_tree() {
    let many: Vec<Node> = (0..8)
        .map(|i| stream(&format!("s{i}"), &pattern(i, 20)))
        .collect();
    let mut bd = Builder::new(3, many);
    bd.degenerate_tree = true;
    let mut b = bd.build();
    // Last node of the right-leaning list points back to the first.
    let first = b.id_of("s7");
    let last = b.id_of("s0");
    let o = b.entry_off(last) + 72;
    b.set_u32(o, first);
    let c = open(&b);
    assert!(find_anomaly(&c, |a| matches!(
        a,
        Anomaly::EntryReferencedTwice(_)
    )));
    assert_eq!(c.list().len(), 8);
    // Self reference through the left link, and through the root.
    let mut b2 = bd_clone(true);
    let last = b2.id_of("s0");
    let o = b2.entry_off(last) + 68;
    b2.set_u32(o, last);
    let c = open(&b2);
    assert!(find_anomaly(&c, |a| *a == Anomaly::EntryReferencedTwice(last)));
    assert_eq!(c.list().len(), 8);
    let mut b3 = bd_clone(true);
    let last = b3.id_of("s0");
    let o = b3.entry_off(last) + 72;
    b3.set_u32(o, 0);
    let c = open(&b3);
    assert!(find_anomaly(&c, |a| *a == Anomaly::EntryReferencedTwice(0)));
    assert_eq!(c.list().len(), 8);
}

fn bd_clone(degenerate: bool) -> Built {
    let many: Vec<Node> = (0..8)
        .map(|i| stream(&format!("s{i}"), &pattern(i, 20)))
        .collect();
    let mut bd = Builder::new(3, many);
    bd.degenerate_tree = degenerate;
    bd.build()
}

#[test]
fn storage_child_pointing_at_an_ancestor() {
    let mut b = sample(3);
    b.set_entry_u32("Dir/Sub", 76, b.id_of("Dir"));
    let c = open(&b);
    assert!(find_anomaly(&c, |a| matches!(
        a,
        Anomaly::EntryReferencedTwice(_)
    )));
    assert_all_readable(&c, &["Dir/Sub/Deep"]);
    assert!(!c.exists("Dir/Sub/Deep"));
    let mut b = sample(3);
    b.set_entry_u32("Dir", 76, 0);
    let c = open(&b);
    assert!(find_anomaly(&c, |a| *a == Anomaly::EntryReferencedTwice(0)));
    assert_eq!(c.read("Big").unwrap(), pattern(4, 20_000));
}

#[test]
fn entry_referenced_by_two_storages() {
    let mut b = Builder::new(
        3,
        vec![
            storage("A", vec![stream("x", b"ax")]),
            storage("B", vec![stream("y", b"by")]),
            stream("ok", b"ok"),
        ],
    )
    .build();
    let a_child = {
        let o = b.entry_off(b.id_of("A")) + 76;
        get_u32(&b.bytes, o)
    };
    b.set_entry_u32("B", 76, a_child);
    let c = open(&b);
    assert!(find_anomaly(&c, |a| *a == Anomaly::EntryReferencedTwice(a_child)));
    // Exactly one of the two storages keeps the shared entry.
    assert!(c.exists("A/x") != c.exists("B/x"));
    assert_eq!(c.read("ok").unwrap(), b"ok");
}

#[test]
fn links_outside_the_directory() {
    let mut b = sample(3);
    b.set_entry_u32("Dir", 76, 1_000_000);
    let c = open(&b);
    assert!(find_anomaly(&c, |a| matches!(a, Anomaly::InvalidLink(_))));
    assert_eq!(c.read("Big").unwrap(), pattern(4, 20_000));
    assert!(c.children(c.find("Dir").unwrap()).next().is_none());
}

fn bad_name_case(mutate: impl Fn(&mut Built, usize)) {
    for v in [3, 4] {
        let mut b = Builder::new(v, vec![stream("Bad", b"b"), stream("Good", b"g")]).build();
        let id = b.id_of("Bad");
        let o = b.entry_off(id);
        mutate(&mut b, o);
        let c = open(&b);
        assert!(
            find_anomaly(&c, |a| *a == Anomaly::InvalidName(id)),
            "{:?}",
            c.anomalies()
        );
        assert!(!c.exists("Bad"));
        assert_eq!(c.read("Good").unwrap(), b"g");
    }
}

#[test]
fn invalid_names() {
    bad_name_case(|b, o| b.set_u16(o + 64, 9)); // odd length
    bad_name_case(|b, o| b.set_u16(o + 64, 66)); // longer than 32 units
    bad_name_case(|b, o| b.set_u16(o + 64, 0)); // empty
    bad_name_case(|b, o| b.set_u16(o + 64, 2 * 3)); // cuts the name, terminator is 'd'...
    bad_name_case(|b, o| b.set_u16(o + 6, u16::from(b'x'))); // no terminator
    bad_name_case(|b, o| b.set_u16(o + 2, 0)); // NUL inside
    bad_name_case(|b, o| b.set_u16(o, u16::from(b'/'))); // separator
    bad_name_case(|b, o| b.set_u16(o, 0xd800)); // lone surrogate
    bad_name_case(|b, o| b.set_u16(o + 2, 0xdc00)); // reversed pair
}

#[test]
fn invalid_types_and_colors() {
    for ty in [0u8, 3, 4, 7, 0xff] {
        let mut b = Builder::new(3, vec![stream("Bad", b"b"), stream("Good", b"g")]).build();
        let id = b.id_of("Bad");
        let o = b.entry_off(id) + 66;
        b.bytes[o] = ty;
        let c = open(&b);
        assert!(
            find_anomaly(&c, |a| *a == Anomaly::InvalidEntryType(id)),
            "{ty}"
        );
        assert_eq!(c.read("Good").unwrap(), b"g");
    }
    // A second root entry.
    let mut b = Builder::new(3, vec![stream("Bad", b"b"), stream("Good", b"g")]).build();
    let id = b.id_of("Bad");
    let o = b.entry_off(id) + 66;
    b.bytes[o] = 5;
    assert!(find_anomaly(&open(&b), |a| *a == Anomaly::InvalidEntryType(id)));
    // Color other than red or black.
    let mut b = Builder::new(3, vec![stream("Bad", b"b"), stream("Good", b"g")]).build();
    let id = b.id_of("Bad");
    let o = b.entry_off(id) + 67;
    b.bytes[o] = 9;
    let c = open(&b);
    assert!(find_anomaly(&c, |a| *a == Anomaly::InvalidColor(id)));
    assert_eq!(c.read("Good").unwrap(), b"g");
}

#[test]
fn duplicate_names_keep_the_first() {
    let b = Builder::new(
        3,
        vec![stream("abc", b"1"), stream("ABC", b"2"), stream("z", b"3")],
    )
    .build();
    let c = open(&b);
    assert!(find_anomaly(&c, |a| matches!(a, Anomaly::DuplicateName(_))));
    assert_eq!(c.list().len(), 2);
    assert_eq!(c.read("z").unwrap(), b"3");
}

#[test]
fn simple_uppercase_mapping_detects_greek_names() {
    let b = Builder::new(
        3,
        vec![stream("\u{1f80}", b"lower"), stream("\u{1f88}", b"upper")],
    )
    .build();
    let c = open(&b);
    assert!(find_anomaly(&c, |a| matches!(a, Anomaly::DuplicateName(_))));
    assert_eq!(c.list().len(), 1);
    assert_eq!(c.read("\u{1f80}").unwrap(), b"lower");
    assert_eq!(c.read("\u{1f88}").unwrap(), b"lower");
}

#[test]
fn stream_child_and_ignored_metadata_are_nonfatal() {
    let mut b = sample(3);
    let stream_id = b.id_of("Small");
    b.set_entry_u32("Small", 76, b.id_of("Empty"));
    let entry = b.entry_off(stream_id);
    b.bytes[entry + 80] = 1; // CLSID is unused for streams.
    b.set_u64(entry + 100, 1); // timestamps are unused for streams.
    let storage_id = b.id_of("Dir");
    let storage = b.entry_off(storage_id);
    b.set_u32(storage + 116, 123);
    b.set_u64(storage + 120, 456);
    let root = b.entry_off(0);
    b.set_u64(root + 100, 1);

    let c = open(&b);
    assert!(
        c.anomalies()
            .contains(&Anomaly::StreamHasChildren(stream_id))
    );
    assert!(
        c.anomalies()
            .contains(&Anomaly::IgnoredStreamMetadata(stream_id))
    );
    assert!(
        c.anomalies()
            .contains(&Anomaly::StorageHasStreamFields(storage_id))
    );
    assert!(c.anomalies().contains(&Anomaly::RootHasCreationTime(0)));
    assert_eq!(c.read("Small").unwrap(), pattern(1, 100));
    assert_eq!(c.read("Empty").unwrap(), b"");
    assert_eq!(c.read("Dir/Inner").unwrap(), pattern(5, 700));
}

// ---------------------------------------------------------------- limits

#[test]
fn configurable_limits() {
    let b = sample(3);
    let l = Limits {
        max_stream_size: 1000,
        ..Limits::default()
    };
    let c = Compound::open(&b.bytes, l).unwrap();
    assert!(matches!(c.read("Big"), Err(CfbError::LimitExceeded { .. })));
    assert!(matches!(
        c.read_range("Big", 0, 1),
        Err(CfbError::LimitExceeded { .. })
    ));
    assert_eq!(c.read("Small").unwrap(), pattern(1, 100));

    let l = Limits {
        max_total_read: 150,
        ..Limits::default()
    };
    let c = Compound::open(&b.bytes, l).unwrap();
    assert!(c.read("Small").is_ok());
    assert!(matches!(
        c.read("Small"),
        Err(CfbError::LimitExceeded { .. })
    ));
    assert!(c.read_range("Small", 0, 50).is_ok());
    assert!(c.read_range("Small", 0, 1).is_err());

    let l = Limits {
        max_file_size: 1000,
        ..Limits::default()
    };
    assert!(matches!(
        Compound::open(&b.bytes, l),
        Err(CfbError::LimitExceeded { .. })
    ));
}

trait CloneBytes {
    fn clone_bytes(&self) -> Vec<u8>;
}
impl CloneBytes for Built {
    fn clone_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}
