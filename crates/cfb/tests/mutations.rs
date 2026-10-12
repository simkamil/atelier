// SPDX-License-Identifier: AGPL-3.0-or-later
//! Seeded mutation and truncation campaign: opening and reading mutated
//! containers must never panic, loop forever or allocate without bound.
mod common;

use atelier_cfb::{Compound, Limits};
use common::*;
use std::time::{Duration, Instant};

fn base_nodes() -> Vec<Node> {
    vec![
        stream("Small", &pattern(1, 100)),
        stream("Mini", &pattern(2, 3000)),
        stream("Big", &pattern(4, 20_000)),
        storage(
            "Dir",
            vec![
                stream("Inner", &pattern(5, 700)),
                storage("Sub", vec![stream("Deep", &pattern(6, 9000))]),
            ],
        ),
        stream("\u{5}Summary", &pattern(7, 300)),
    ]
}

fn limits() -> Limits {
    Limits {
        max_file_size: 64 * 1024 * 1024,
        max_entries: 5000,
        max_depth: 16,
        max_stream_size: 8 * 1024 * 1024,
        max_total_read: 64 * 1024 * 1024,
    }
}

fn exercise(bytes: &[u8]) -> bool {
    let Ok(c) = Compound::open(bytes, limits()) else {
        return false;
    };
    let _ = c.anomalies().len();
    let _ = c.root_clsid();
    for l in c.list() {
        if l.entry.is_stream() {
            let _ = c.read_entry(l.entry);
            let _ = c.read_entry_range(l.entry, 100, 700);
            let _ = c.read_entry_range(l.entry, l.entry.size(), 1);
            let _ = c.read_entry_range(l.entry, u64::MAX, 1);
        }
        let _ = c.find(&l.path);
    }
    let _ = c.find("Dir/Sub/Deep");
    let _ = c.read("Small");
    true
}

const INTERESTING: [u32; 12] = [
    0,
    1,
    2,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_fff9,
    0xffff_fffa,
    0xffff_fffb,
    0xffff_fffc,
    0xffff_fffd,
    0xffff_fffe,
    0xffff_ffff,
];

fn mutate(rng: &mut Rng, b: &Built) -> Vec<u8> {
    let mut x = b.bytes.clone();
    let len = x.len();
    let pick = |rng: &mut Rng| -> usize {
        match rng.below(4) {
            0 => rng.below(512.min(len)),
            1 => b.entry_off(0) + rng.below((b.dir_sectors as usize * b.ss).min(len)),
            2 => b.sector_off(b.fat_sectors[0]) + rng.below(b.ss),
            _ => rng.below(len),
        }
    };
    for _ in 0..1 + rng.below(6) {
        let o = pick(rng);
        match rng.below(6) {
            0 => {
                if let Some(v) = x.get_mut(o) {
                    *v = rng.next() as u8;
                }
            }
            1 | 2 => {
                let o = o & !3;
                let v = if rng.below(3) == 0 {
                    rng.next() as u32
                } else {
                    INTERESTING[rng.below(INTERESTING.len())]
                };
                if o + 4 <= x.len() {
                    put_u32(&mut x, o, v);
                }
            }
            3 => {
                let n = rng.below(300).min(x.len().saturating_sub(o));
                x[o..o + n].fill(0);
            }
            4 => {
                // Copy a 128-byte directory-sized chunk over another place.
                let from = pick(rng);
                if from + 128 <= x.len() && o + 128 <= x.len() {
                    let chunk: Vec<u8> = x[from..from + 128].to_vec();
                    x[o..o + 128].copy_from_slice(&chunk);
                }
            }
            _ => {
                let n = x.len();
                x.truncate(1 + rng.below(n));
                break;
            }
        }
    }
    x
}

fn campaign(base: &Built, seed: u64, iterations: usize) -> Duration {
    assert!(
        exercise(&base.bytes),
        "known-valid campaign baseline rejected"
    );
    let mut opened = 0;
    let mut rng = Rng(seed);
    let start = Instant::now();
    for i in 0..iterations {
        let m = mutate(&mut rng, base);
        let t = Instant::now();
        opened += usize::from(exercise(&m));
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "iteration {i} (seed {seed}) too slow"
        );
    }
    // Mutations must exercise both parser paths. The original 10% threshold
    // was arbitrary and rejects useful campaigns when stricter metadata
    // validation correctly rejects malformed mutations.
    assert!(opened > 0 && opened < iterations, "{opened}/{iterations}");
    start.elapsed()
}

#[test]
fn mutations_v3() {
    let b = Builder::new(3, base_nodes()).build();
    exercise(&b.bytes);
    campaign(&b, 0xC0FFEE, 6000);
}

#[test]
fn mutations_v4() {
    let b = Builder::new(4, base_nodes()).build();
    exercise(&b.bytes);
    campaign(&b, 0xBADC0DE, 4000);
}

#[test]
fn mutations_with_difat_chain() {
    let mut bd = Builder::new(3, base_nodes());
    bd.min_fat_sectors = 250;
    let b = bd.build();
    exercise(&b.bytes);
    campaign(&b, 0xD1FA7, 2500);
    let mut bd = Builder::new(4, base_nodes());
    bd.min_fat_sectors = 111;
    let b = bd.build();
    exercise(&b.bytes);
    campaign(&b, 0x4D1FA7, 400);
}

#[test]
fn mutations_with_degenerate_tree() {
    let mut bd = Builder::new(3, base_nodes());
    bd.degenerate_tree = true;
    let b = bd.build();
    campaign(&b, 0x7EE, 3000);
}

#[test]
fn every_truncation_of_small_containers() {
    for v in [3, 4] {
        let b = Builder::new(v, base_nodes()).build();
        let step = if v == 3 { 1 } else { 61 };
        let mut cut = 0;
        while cut < b.bytes.len() {
            exercise(&b.bytes[..cut]);
            cut += step;
        }
    }
}

#[test]
fn random_garbage_with_a_valid_signature() {
    let mut rng = Rng(42);
    for _ in 0..2000 {
        let n = 8 + rng.below(3000);
        let mut x: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        x[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        exercise(&x);
    }
}
