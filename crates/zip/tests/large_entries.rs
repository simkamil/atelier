// SPDX-License-Identifier: AGPL-3.0-or-later
//! Full ZIP round trips of multi-megabyte entries (regression: a 2.5 MiB or
//! larger repetitive XML part used to fail with `InvalidDeflate`).
use atelier_zip::{Archive, Limits, Method, WriteOptions, Writer};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn xml(size: usize) -> Vec<u8> {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < size {
        s.push_str(&format!(
            "<w:p><w:bookmarkStart w:id=\"{i}\" w:name=\"b{i}\"/><w:proofErr w:type=\"spellStart\"/>\
             <w:r><w:t>Paragraph {i}</w:t></w:r><w:proofErr w:type=\"spellEnd\"/>\
             <w:bookmarkEnd w:id=\"{i}\"/></w:p>"
        ));
        i += 1;
    }
    s.into_bytes()
}

fn skewed(size: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    (0..size)
        .map(|_| (r.next().trailing_zeros() as u8).min(40))
        .collect()
}

fn round_trip(name: &str, data: &[u8]) {
    let mut w = Writer::new(WriteOptions::default());
    w.add("part.xml", data, Method::Deflate).unwrap();
    let zip = w.finish().unwrap();
    let limits = Limits {
        max_compression_ratio: u64::MAX,
        ..Limits::default()
    };
    let archive = Archive::open(&zip, limits).unwrap();
    let back = archive.entry("part.xml").unwrap().read_all();
    assert!(
        back.as_deref() == Ok(data),
        "{name}: {:?}",
        back.map(|b| b.len())
    );
}

#[test]
fn repetitive_xml_part_of_every_size_round_trips() {
    for mib in [1.0, 2.0, 2.5, 2.9, 3.0, 4.0] {
        round_trip("xml", &xml((mib * 1_048_576.0) as usize));
    }
}

#[test]
fn varied_contents_round_trip() {
    let mut r = Rng(42);
    round_trip("zeros", &vec![0u8; 3_000_000]);
    round_trip("byte", &vec![7u8; 3_000_000]);
    round_trip("skewed", &skewed(3_000_000, 5));
    let random: Vec<u8> = (0..1_000_000).map(|_| r.next() as u8).collect();
    round_trip("random", &random);
    let period: Vec<u8> = (0..3_000_000).map(|i| random[i % 65_536]).collect();
    round_trip("period 65536", &period);
}

#[test]
#[ignore = "heavy: 8 and 16 MiB"]
fn heavy_entries_round_trip() {
    for mib in [8usize, 16] {
        round_trip("xml", &xml(mib << 20));
    }
}
