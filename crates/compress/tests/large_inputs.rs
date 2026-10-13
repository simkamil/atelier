// SPDX-License-Identifier: AGPL-3.0-or-later
//! Round trips of multi-megabyte inputs. Deep Huffman trees (skewed symbol
//! frequencies over a large input) once produced over-subscribed codes.
use atelier_compress::{
    CompressError, Format, InflateState, Level, Status, compress, compress_zlib,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn deflate(input: &[u8], level: Level, zlib: bool) -> Vec<u8> {
    let mut out = vec![0u8; input.len() / 2 + 64];
    let f = if zlib { compress_zlib } else { compress };
    let n = match f(input, level, &mut out) {
        Ok(n) => n,
        Err(CompressError::OutputTooSmall { needed }) => {
            out.resize(needed, 0);
            f(input, level, &mut out).unwrap()
        }
    };
    out.truncate(n);
    out
}

fn inflate(bytes: &[u8], format: Format) -> Vec<u8> {
    let mut state = InflateState::new(format, u64::MAX);
    let mut decoded = Vec::new();
    let mut pos = 0;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let p = state
            .push(&bytes[pos..], &mut buf, true)
            .expect("valid stream");
        pos += p.consumed;
        decoded.extend_from_slice(&buf[..p.produced]);
        if p.status == Status::Done {
            assert_eq!(pos, bytes.len());
            return decoded;
        }
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

fn periodic(size: usize, period: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    let unit: Vec<u8> = (0..period).map(|_| r.next() as u8).collect();
    (0..size).map(|i| unit[i % period]).collect()
}

fn random(size: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    (0..size).map(|_| r.next() as u8).collect()
}

fn mixed(size: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        let n = (r.next() % 70_000) as usize + 1;
        match r.next() % 4 {
            0 => out.extend((0..n).map(|_| r.next() as u8)),
            1 => out.extend(std::iter::repeat_n(r.next() as u8, n)),
            2 => {
                let p = (r.next() % 70_000) as usize + 1;
                out.extend(periodic(n, p, r.next() | 1));
            }
            _ => {
                // Geometrically skewed alphabet: forces codes deeper than 15.
                for _ in 0..n {
                    out.push((r.next().trailing_zeros() as u8).min(40));
                }
            }
        }
    }
    out.truncate(size);
    out
}

fn check(name: &str, data: &[u8]) {
    for level in [Level::Fast, Level::Normal] {
        let raw = deflate(data, level, false);
        assert!(
            inflate(&raw, Format::Deflate) == data,
            "{name} {level:?} raw"
        );
        let z = deflate(data, level, true);
        assert!(inflate(&z, Format::Zlib) == data, "{name} {level:?} zlib");
    }
}

#[test]
fn xml_past_two_mebibytes_round_trips() {
    check("xml 2.5 MiB", &xml(2_621_440));
}

#[test]
fn skewed_alphabet_beyond_fifteen_bits_round_trips() {
    // Fibonacci-spread symbol counts need more than 15 bits in an
    // unrestricted Huffman tree.
    let mut data = Vec::new();
    let (mut a, mut b) = (1usize, 1usize);
    for sym in 0..24u8 {
        data.extend(std::iter::repeat_n(sym, a));
        let c = a + b;
        a = b;
        b = c;
        if data.len() > 1_500_000 {
            break;
        }
    }
    check("fibonacci", &data);
}

#[test]
fn fixed_seed_property_over_sizes_and_contents() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..10 {
        let size = match case % 5 {
            0 => 140_000,
            1 => 200_000,
            2 => 400_000,
            3 => 131_073,
            _ => (r.next() % 500_000) as usize + 1,
        };
        let period = [
            1, 2, 3, 258, 32_767, 32_768, 32_769, 65_535, 65_536, 65_537,
        ][case % 10];
        let seed = r.next() | 1;
        check("periodic", &periodic(size, period, seed));
        check("random", &random(size.min(100_000), seed));
        check("mixed", &mixed(size, seed));
    }
}

#[test]
fn uniform_inputs_round_trip() {
    for size in [0, 1, 258, 259, 65_535, 65_536, 1_000_000] {
        check("zeros", &vec![0u8; size]);
        check("byte", &vec![0xA5u8; size]);
    }
}

#[test]
#[ignore = "heavy: tens of megabytes"]
fn heavy_sizes_round_trip() {
    for mb in [4usize, 8, 16] {
        check("xml", &xml(mb << 20));
        check("mixed", &mixed(mb << 20, mb as u64));
        check("periodic", &periodic(mb << 20, 32_768, 7));
    }
}
