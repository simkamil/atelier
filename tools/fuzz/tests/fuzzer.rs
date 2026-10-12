// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_fuzz::mutate::{self, Context, INTERESTING, Mutation};
use atelier_fuzz::rng::Rng;
use atelier_fuzz::{
    Config, FailureKind, FuzzTarget, Outcome, Runner, find, fuzz, minimize, registry,
};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-fuzz-test-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx<'a>(other: Option<&'a [u8]>, dictionary: &'a [&'a [u8]], max_len: usize) -> Context<'a> {
    Context {
        other,
        dictionary,
        max_len,
    }
}

// ------------------------------------------------------------ generator

#[test]
fn generator_is_reproducible_and_seed_sensitive() {
    let draw = |seed| {
        let mut rng = Rng::new(seed);
        (0..32).map(|_| rng.next_u64()).collect::<Vec<_>>()
    };
    assert_eq!(draw(7), draw(7));
    assert_ne!(draw(7), draw(8));
    let mut rng = Rng::new(1);
    assert_eq!(rng.below(0), 0);
    assert!((0..1000).all(|_| rng.below(5) < 5));
}

// ------------------------------------------------------------ mutations

#[test]
fn stacked_mutations_are_reproducible_from_the_seed() {
    let run = |seed| {
        let mut rng = Rng::new(seed);
        let tokens: [&[u8]; 2] = [b"AB", b"<?xml"];
        let other = b"another corpus entry".to_vec();
        let mut outputs = Vec::new();
        for _ in 0..200 {
            let mut data = b"0123456789abcdef".to_vec();
            let applied = mutate::mutate(&mut rng, &mut data, &ctx(Some(&other), &tokens, 64));
            outputs.push((data, applied));
        }
        outputs
    };
    assert_eq!(run(42), run(42));
    assert_ne!(run(42), run(43));
    for (data, applied) in run(42) {
        assert!(data.len() <= 64);
        assert!((1..=5).contains(&applied.len()) || applied.is_empty());
    }
}

#[test]
fn bit_flip_changes_one_to_three_bits() {
    let mut rng = Rng::new(1);
    for _ in 0..200 {
        let original = vec![0u8; 12];
        let mut data = original.clone();
        assert!(mutate::bit_flip(&mut rng, &mut data));
        assert_eq!(data.len(), original.len());
        let bits: u32 = data.iter().map(|b| b.count_ones()).sum();
        assert!(bits <= 3, "{bits} bits set");
    }
    assert!(!mutate::bit_flip(&mut rng, &mut []));
}

#[test]
fn interesting_values_cover_all_widths_and_both_byte_orders() {
    let mut rng = Rng::new(3);
    let mut widths = HashSet::new();
    let (mut little, mut big) = (false, false);
    for _ in 0..2000 {
        let mut data = vec![0x55u8; 16];
        assert!(mutate::interesting(&mut rng, &mut data));
        let changed: Vec<usize> = (0..16).filter(|&i| data[i] != 0x55).collect();
        let width = changed.len();
        assert!([1, 2, 4, 8].contains(&width), "width {width}");
        assert_eq!(changed[width - 1] - changed[0], width - 1, "contiguous");
        let field = &data[changed[0]..=changed[width - 1]];
        let values = INTERESTING.iter().find(|(w, _)| *w == width).unwrap().1;
        let le = values.iter().any(|v| v.to_le_bytes()[..width] == *field);
        let be = values
            .iter()
            .any(|v| v.to_be_bytes()[8 - width..] == *field);
        assert!(le || be, "{field:?} is not a boundary value");
        if le && !be {
            little = true;
        }
        if be && !le {
            big = true;
        }
        widths.insert(width);
    }
    assert_eq!(widths.len(), 4);
    assert!(little && big);
}

#[test]
fn insert_delete_duplicate_respect_sizes_and_limits() {
    let mut rng = Rng::new(5);
    for _ in 0..300 {
        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::insert(&mut rng, &mut data, 64));
        assert!((11..=26).contains(&data.len()));

        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::delete(&mut rng, &mut data));
        assert!(data.len() < 10);

        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::duplicate(&mut rng, &mut data, 64));
        assert!(data.len() > 10 && data.len() <= 20);
        // Every original byte is still there, in order, plus a copied block.
        let mut originals = b"abcdefghij".iter();
        let mut next = originals.next();
        for b in &data {
            if Some(b) == next {
                next = originals.next();
            }
        }
        assert!(next.is_none());
    }
    let mut full = vec![1u8; 8];
    assert!(!mutate::insert(&mut rng, &mut full, 8));
    assert!(!mutate::duplicate(&mut rng, &mut full, 8));
    assert!(!mutate::delete(&mut rng, &mut Vec::new()));
}

#[test]
fn splice_dictionary_and_truncate() {
    let mut rng = Rng::new(9);
    let other = b"0123456789".to_vec();
    for _ in 0..200 {
        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::splice(&mut rng, &mut data, &other, 100));
        let prefix = data.iter().take_while(|b| b.is_ascii_lowercase()).count();
        assert_eq!(&data[..prefix], &b"abcdefghij"[..prefix]);
        assert!(other.ends_with(&data[prefix..]) && prefix + 10 >= data.len());
    }
    assert!(!mutate::splice(&mut rng, &mut vec![1], &[], 10));

    let token: &[u8] = b"<![CDATA[";
    for _ in 0..200 {
        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::dictionary(&mut rng, &mut data, &[token], 100));
        assert!(data.windows(token.len()).any(|w| w == token));
    }
    assert!(!mutate::dictionary(&mut rng, &mut vec![1], &[], 10));

    for _ in 0..200 {
        let mut data = b"abcdefghij".to_vec();
        assert!(mutate::truncate(&mut rng, &mut data));
        assert!(data.len() < 10);
    }
    assert!(!mutate::truncate(&mut rng, &mut vec![1]));
}

#[test]
fn every_mutation_kind_is_dispatched() {
    let mut rng = Rng::new(11);
    let other = b"zzzz".to_vec();
    let tokens: [&[u8]; 1] = [b"TOK"];
    for kind in mutate::ALL {
        let mut applied = false;
        for _ in 0..20 {
            let mut data = b"hello world".to_vec();
            applied |= mutate::apply(kind, &mut rng, &mut data, &ctx(Some(&other), &tokens, 99));
        }
        assert!(applied, "{kind:?} never applied");
    }
    let mut data = b"abc".to_vec();
    assert!(!mutate::apply(
        Mutation::Splice,
        &mut rng,
        &mut data,
        &ctx(None, &[], 9)
    ));
}

// ------------------------------------------------------------- targets

/// Panics when the input contains `FUZZ!`.
struct Faulty;

impl FuzzTarget for Faulty {
    fn name(&self) -> &str {
        "faulty"
    }
    fn run(&self, input: &[u8]) -> Outcome {
        if input.windows(5).any(|w| w == b"FUZZ!") {
            panic!("pattern reached");
        }
        Outcome::accepted(input.len() as u64 % 5)
    }
    fn dictionary(&self) -> &[&'static [u8]] {
        &[b"FUZZ!", b"noise"]
    }
    fn seeds(&self) -> Vec<Vec<u8>> {
        vec![b"hello".to_vec(), b"seed two".to_vec()]
    }
}

/// Never returns on inputs starting with `H`, until released.
struct Hang(Arc<AtomicBool>);

impl FuzzTarget for Hang {
    fn name(&self) -> &str {
        "hang"
    }
    fn run(&self, input: &[u8]) -> Outcome {
        if input.first() == Some(&b'H') {
            while !self.0.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Outcome::accepted(0)
    }
    fn seeds(&self) -> Vec<Vec<u8>> {
        vec![b"ok".to_vec(), b"Hang".to_vec()]
    }
}

struct Greedy;

impl FuzzTarget for Greedy {
    fn name(&self) -> &str {
        "greedy"
    }
    fn run(&self, input: &[u8]) -> Outcome {
        Outcome::accepted(if input.len() > 3 { 1 << 40 } else { 1 })
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Vec<u8>>>);

impl FuzzTarget for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }
    fn run(&self, input: &[u8]) -> Outcome {
        self.0.lock().unwrap().push(input.to_vec());
        Outcome::labelled(
            &format!("first-{}", input.first().copied().unwrap_or(0) % 7),
            input.len() as u64,
        )
    }
    fn dictionary(&self) -> &[&'static [u8]] {
        &[b"tok"]
    }
    fn seeds(&self) -> Vec<Vec<u8>> {
        vec![b"alpha".to_vec(), b"beta beta".to_vec()]
    }
}

fn config(seed: u64, iterations: u64) -> Config {
    Config {
        seed,
        iterations: Some(iterations),
        timeout: Duration::from_secs(5),
        ..Config::default()
    }
}

#[test]
fn same_seed_generates_the_same_inputs() {
    let run = |seed| {
        let recorder = Arc::new(Recorder::default());
        let summary = fuzz(recorder.clone(), &config(seed, 1500)).unwrap();
        let inputs = recorder.0.lock().unwrap().clone();
        (summary.corpus_size, summary.iterations, inputs)
    };
    let (corpus_a, iterations_a, inputs_a) = run(5);
    let (corpus_b, iterations_b, inputs_b) = run(5);
    assert_eq!((corpus_a, iterations_a), (corpus_b, iterations_b));
    assert_eq!(inputs_a, inputs_b);
    assert_eq!(iterations_a, 1500);
    assert!(corpus_a > 2, "new outcomes must grow the corpus");
    assert_ne!(inputs_a, run(6).2);
}

#[test]
fn finds_saves_deduplicates_and_minimizes_a_panic() {
    let dir = temp_dir("panic");
    let mut cfg = config(1, 20_000);
    cfg.crashes_dir = Some(dir.clone());
    cfg.minimize = true;
    let summary = fuzz(Arc::new(Faulty), &cfg).unwrap();

    assert_eq!(summary.failures.len(), 1, "one panic location, one failure");
    let found = &summary.failures[0];
    assert_eq!(found.failure.kind, FailureKind::Panic);
    assert_eq!(found.failure.message, "pattern reached");
    assert!(
        found.failure.location.contains("fuzzer.rs"),
        "{}",
        found.failure.location
    );
    assert!(
        summary.duplicate_failures > 0,
        "later hits are deduplicated"
    );
    assert!(!summary.hung);
    assert_eq!(found.minimized.as_deref(), Some(&b"FUZZ!"[..]));

    // The crash folder holds the input, its minimized form and a note.
    let saved = found.saved.as_ref().unwrap();
    assert_eq!(fs::read(saved).unwrap(), found.input);
    assert!(saved.starts_with(dir.join("faulty")));
    let stem = saved.file_stem().unwrap().to_string_lossy().to_string();
    assert!(stem.starts_with("panic-"));
    let min = fs::read(dir.join("faulty").join(format!("{stem}.min.bin"))).unwrap();
    assert_eq!(min, b"FUZZ!");
    let note = fs::read_to_string(dir.join("faulty").join(format!("{stem}.txt"))).unwrap();
    assert!(
        note.contains("seed: 1") && note.contains("pattern reached"),
        "{note}"
    );

    // Same seed, same defect, same file name.
    let again = fuzz(Arc::new(Faulty), &cfg).unwrap();
    assert_eq!(again.failures[0].iteration, found.iteration);
    assert_eq!(again.failures[0].saved, found.saved);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn minimize_reduces_to_the_triggering_pattern_only() {
    let mut runner = Runner::new(Arc::new(Faulty), Duration::from_secs(5), u64::MAX);
    let noisy: Vec<u8> = (0..300u32)
        .map(|i| (i % 200) as u8)
        .chain(*b"FUZZ!")
        .chain((0..100u32).map(|i| i as u8))
        .collect();
    let failure = runner.run(&noisy).unwrap_err();
    assert_eq!(minimize(&mut runner, &noisy, &failure, 20_000), b"FUZZ!");
    // A budget of zero returns the input untouched.
    assert_eq!(minimize(&mut runner, &noisy, &failure, 0), noisy);
}

#[test]
fn replay_reproduces_the_same_failure() {
    let mut runner = Runner::new(Arc::new(Faulty), Duration::from_secs(5), u64::MAX);
    let first = runner.run(b"xxFUZZ!yy").unwrap_err();
    let second = runner.run(b"xxFUZZ!yy").unwrap_err();
    assert_eq!(first, second);
    assert!(runner.run(b"harmless").is_ok(), "worker survives a panic");
}

#[test]
fn detects_a_hang_without_blocking_and_stops_the_run() {
    let release = Arc::new(AtomicBool::new(false));
    let dir = temp_dir("hang");
    let mut cfg = config(1, 1000);
    cfg.timeout = Duration::from_millis(150);
    cfg.crashes_dir = Some(dir.clone());
    let summary = fuzz(Arc::new(Hang(release.clone())), &cfg).unwrap();
    release.store(true, Ordering::Relaxed);

    assert!(summary.hung);
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(summary.failures[0].failure.kind, FailureKind::Timeout);
    assert_eq!(summary.failures[0].failure.key(), "timeout");
    assert!(summary.iterations < 1000, "the run stops at the hang");
    let saved = summary.failures[0].saved.as_ref().unwrap();
    assert_eq!(fs::read(saved).unwrap(), b"Hang");
    assert!(
        saved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("timeout-")
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn runner_recovers_after_a_timeout() {
    let release = Arc::new(AtomicBool::new(false));
    let mut runner = Runner::new(
        Arc::new(Hang(release.clone())),
        Duration::from_millis(100),
        u64::MAX,
    );
    assert_eq!(runner.run(b"Hang").unwrap_err().kind, FailureKind::Timeout);
    assert_eq!(runner.abandoned_workers(), 1);
    assert!(runner.run(b"fine").is_ok());
    release.store(true, Ordering::Relaxed);
}

#[test]
fn reported_output_over_budget_is_a_memory_failure() {
    let mut runner = Runner::new(Arc::new(Greedy), Duration::from_secs(5), 1 << 20);
    assert!(runner.run(b"ab").is_ok());
    let failure = runner.run(b"abcd").unwrap_err();
    assert_eq!(failure.kind, FailureKind::Memory);
    assert!(failure.message.contains("budget"));
}

// ------------------------------------------------------ real analyzers

#[test]
fn registry_lists_the_analyzers_once_each() {
    let names: Vec<String> = registry().iter().map(|t| t.name().to_string()).collect();
    let unique: HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
    for expected in [
        "inflate",
        "zlib",
        "zip",
        "xml",
        "png",
        "gif",
        "bmp",
        "jpeg",
        "opentype",
        "truetype",
        "cff",
        "opc",
        "encodings",
        "display-list",
        "recovery-journal",
    ] {
        assert!(find(expected).is_some(), "missing target {expected}");
    }
    assert!(
        names
            .iter()
            .all(|n| !n.contains("wasm") && !n.contains("desktop"))
    );
    assert!(find("nope").is_none());
}

#[test]
fn builtin_seeds_run_cleanly_and_degenerate_inputs_do_not_panic() {
    for target in registry() {
        let mut runner = Runner::new(target.clone(), Duration::from_secs(10), 256 << 20);
        let seeds = target.seeds();
        assert!(!seeds.is_empty(), "{} has no seed", target.name());
        for seed in &seeds {
            assert!(
                seed.len() <= 8192,
                "{}: seed too large for the repository",
                target.name()
            );
            runner
                .run(seed)
                .unwrap_or_else(|f| panic!("{}: seed fails: {}", target.name(), f.describe()));
        }
        for input in [&[][..], &[0], &[0xFF; 3], &[0u8; 64]] {
            runner.run(input).unwrap_or_else(|f| {
                panic!("{}: {:?} fails: {}", target.name(), input, f.describe())
            });
        }
    }
}

#[test]
fn well_formed_seeds_are_accepted_by_their_analyzer() {
    for name in ["png", "gif", "bmp", "display-list"] {
        let target = find(name).unwrap();
        for (i, seed) in target.seeds().iter().enumerate() {
            assert_eq!(target.run(seed).class, 0, "{name} seed {i} was rejected");
        }
    }
    for name in ["jpeg", "inflate", "zlib"] {
        let target = find(name).unwrap();
        assert!(
            target
                .seeds()
                .iter()
                .any(|seed| target.run(seed).class == 0),
            "{name} accepts none of its seeds"
        );
    }
}

#[test]
fn short_campaign_over_every_target_finds_nothing() {
    for target in registry() {
        let mut cfg = config(2024, 400);
        cfg.max_len = 8192;
        let summary = fuzz(target.clone(), &cfg).unwrap();
        assert!(
            summary.failures.is_empty(),
            "{}: {}",
            target.name(),
            summary.failures[0].failure.describe()
        );
    }
}
