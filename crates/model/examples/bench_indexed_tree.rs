// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reproducible insertion benchmark for `IndexedTree`.

use atelier_model::IndexedTree;
use std::hint::black_box;
use std::time::{Duration, Instant};

const FIXTURE_SIZE: usize = 100_000;
const INSERTIONS: usize = 10_000;
const REQUIRED_SAMPLES: usize = 5;
const LIMIT: Duration = Duration::from_millis(50);

fn insertion_positions() -> Vec<usize> {
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    let mut positions = Vec::with_capacity(INSERTIONS);
    for offset in 0..INSERTIONS {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        positions.push((state as usize) % (FIXTURE_SIZE + offset + 1));
    }
    positions
}

fn fixture() -> IndexedTree<usize> {
    let mut tree = IndexedTree::new();
    for value in 0..FIXTURE_SIZE {
        tree.insert(value, value)
            .expect("fixture insertion is valid");
    }
    tree
}

fn measure(positions: &[usize]) -> Duration {
    let mut tree = fixture();
    let started = Instant::now();
    let mut checksum = 0_u64;
    for (offset, &position) in positions.iter().enumerate() {
        let id = tree
            .insert(position, black_box(FIXTURE_SIZE + offset))
            .expect("precomputed insertion position is valid");
        black_box(id);
        checksum = checksum.wrapping_add(offset as u64);
    }
    black_box((checksum, tree.len()));
    started.elapsed()
}

fn main() {
    let positions = insertion_positions();
    let warmup = measure(&positions);
    let mut samples = Vec::with_capacity(REQUIRED_SAMPLES);
    for _ in 0..REQUIRED_SAMPLES {
        samples.push(measure(&positions));
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let passed = median < LIMIT;
    println!(
        "IndexedTree insert benchmark: fixture={FIXTURE_SIZE}, operations={INSERTIONS}, \
         samples={REQUIRED_SAMPLES}, warmup={warmup:?}, median={median:?}, limit={LIMIT:?}, \
         result={}, host={}-{}",
        if passed { "PASS" } else { "FAIL" },
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if !passed {
        eprintln!("median exceeds the 50 ms target; this result is host-dependent");
        std::process::exit(1);
    }
}
