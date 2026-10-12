// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fuzzing loop, corpus handling, crash storage and minimization.

use crate::mutate::{self, Context, Mutation};
use crate::rng::{Rng, fnv1a};
use crate::runner::{Failure, FailureKind, Runner};
use crate::target::FuzzTarget;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Upper bound on the in-memory working corpus of one run.
pub const MAX_CORPUS_ENTRIES: usize = 2048;
/// Default maximum number of target executions spent minimizing one input.
pub const DEFAULT_MINIMIZE_RUNS: usize = 20_000;
/// Wall-clock cap on one minimization.
pub const MINIMIZE_MAX_TIME: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub struct Config {
    pub seed: u64,
    /// Stop after this many executions (seeds included).
    pub iterations: Option<u64>,
    /// Stop after this much wall-clock time.
    pub duration: Option<Duration>,
    /// Per-input deadline.
    pub timeout: Duration,
    /// Largest input produced by mutation.
    pub max_len: usize,
    /// Memory budget compared with the output size reported by the target.
    pub max_output: u64,
    /// Directory of extra seed files for this target, if any.
    pub corpus_dir: Option<PathBuf>,
    /// Directory receiving failing inputs, if any.
    pub crashes_dir: Option<PathBuf>,
    /// Also store a minimized copy of each new failure.
    pub minimize: bool,
    /// File rewritten with each input before it runs, so that a crash that
    /// kills the process (stack overflow, allocation failure) leaves its input.
    pub inflight: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 1,
            iterations: Some(10_000),
            duration: None,
            timeout: Duration::from_secs(5),
            max_len: 64 * 1024,
            max_output: 256 * 1024 * 1024,
            corpus_dir: None,
            crashes_dir: None,
            minimize: false,
            inflight: None,
        }
    }
}

/// One distinct failure (by kind and panic location).
#[derive(Clone, Debug)]
pub struct Found {
    pub failure: Failure,
    pub input: Vec<u8>,
    pub iteration: u64,
    pub saved: Option<PathBuf>,
    pub minimized: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub target: String,
    pub seed: u64,
    pub iterations: u64,
    pub elapsed: Duration,
    pub corpus_size: usize,
    /// Distinct failures; repeats of the same key are only counted.
    pub failures: Vec<Found>,
    pub duplicate_failures: u64,
    /// A target did not return before the deadline: the run was stopped.
    pub hung: bool,
}

impl Summary {
    pub fn per_second(&self) -> f64 {
        self.iterations as f64 / self.elapsed.as_secs_f64().max(1e-9)
    }
}

/// Built-in seeds followed by the files of `dir` in name order, without
/// duplicates and cut to `max_len`. Never empty: an empty input is added if
/// there is nothing else.
pub fn load_corpus(
    target: &dyn FuzzTarget,
    dir: Option<&Path>,
    max_len: usize,
) -> io::Result<Vec<Vec<u8>>> {
    let mut entries = target.seeds();
    if let Some(dir) = dir
        && dir.is_dir()
    {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
            })
            .collect();
        paths.sort();
        for path in paths {
            entries.push(fs::read(path)?);
        }
    }
    let mut seen = HashSet::new();
    let mut corpus = Vec::new();
    for mut entry in entries {
        entry.truncate(max_len);
        if seen.insert(entry.clone()) {
            corpus.push(entry);
        }
    }
    if corpus.is_empty() {
        corpus.push(Vec::new());
    }
    Ok(corpus)
}

/// Content fingerprint used in file names.
pub fn fingerprint(input: &[u8]) -> String {
    format!("{:016x}", fnv1a(input))
}

/// Run a campaign on one target.
pub fn fuzz(target: Arc<dyn FuzzTarget>, config: &Config) -> io::Result<Summary> {
    let started = Instant::now();
    let mut runner = Runner::new(target.clone(), config.timeout, config.max_output);
    let mut corpus = load_corpus(&*target, config.corpus_dir.as_deref(), config.max_len)?;
    let mut rng = Rng::new(config.seed);
    let mut seen = HashSet::new();
    let mut keys = HashSet::new();
    let mut summary = Summary {
        target: target.name().to_string(),
        seed: config.seed,
        iterations: 0,
        elapsed: Duration::ZERO,
        corpus_size: 0,
        failures: Vec::new(),
        duplicate_failures: 0,
        hung: false,
    };
    let dictionary: Vec<&[u8]> = target.dictionary().to_vec();
    let initial = corpus.len();
    let mut index = 0usize;
    loop {
        if config.iterations.is_some_and(|n| summary.iterations >= n)
            || config.duration.is_some_and(|d| started.elapsed() >= d)
        {
            break;
        }
        let (data, applied) = if index < initial {
            index += 1;
            (corpus[index - 1].clone(), Vec::new())
        } else {
            let mut data = corpus[rng.below(corpus.len())].clone();
            let other = &corpus[rng.below(corpus.len())];
            let ctx = Context {
                other: Some(other),
                dictionary: &dictionary,
                max_len: config.max_len,
            };
            let applied = mutate::mutate(&mut rng, &mut data, &ctx);
            (data, applied)
        };
        if let Some(path) = &config.inflight {
            fs::write(path, &data)?;
        }
        summary.iterations += 1;
        match runner.run(&data) {
            Ok(outcome) => {
                if seen.insert(outcome.signal()) && corpus.len() < MAX_CORPUS_ENTRIES {
                    corpus.push(data);
                }
            }
            Err(failure) => {
                let hang = failure.kind == FailureKind::Timeout;
                if keys.insert(failure.key()) {
                    let mut found = Found {
                        failure,
                        input: data,
                        iteration: summary.iterations,
                        saved: None,
                        minimized: None,
                    };
                    if let Some(dir) = &config.crashes_dir {
                        found.saved = Some(save_failure(
                            dir,
                            target.name(),
                            config.seed,
                            &found,
                            &applied,
                        )?);
                    }
                    if config.minimize {
                        let reduced = minimize(
                            &mut runner,
                            &found.input,
                            &found.failure,
                            DEFAULT_MINIMIZE_RUNS,
                        );
                        if let Some(saved) = &found.saved {
                            save_minimized(saved, &reduced)?;
                        }
                        found.minimized = Some(reduced);
                    }
                    summary.failures.push(found);
                } else {
                    summary.duplicate_failures += 1;
                }
                if hang {
                    summary.hung = true;
                    break;
                }
            }
        }
    }
    summary.elapsed = started.elapsed();
    summary.corpus_size = corpus.len();
    Ok(summary)
}

/// Write `<dir>/<target>/<kind>-<fingerprint>.bin` and a `.txt` note holding
/// the seed and the reason. Returns the `.bin` path.
fn save_failure(
    dir: &Path,
    target: &str,
    seed: u64,
    found: &Found,
    applied: &[Mutation],
) -> io::Result<PathBuf> {
    let folder = dir.join(target);
    fs::create_dir_all(&folder)?;
    let stem = format!(
        "{}-{}",
        found.failure.kind.label(),
        fingerprint(&found.input)
    );
    let path = folder.join(format!("{stem}.bin"));
    fs::write(&path, &found.input)?;
    let note = format!(
        "target: {target}\nkind: {}\nseed: {seed}\niteration: {}\nlocation: {}\nmessage: {}\nmutations: {applied:?}\ninput-bytes: {}\n",
        found.failure.kind.label(),
        found.iteration,
        found.failure.location,
        found.failure.message.replace('\n', " "),
        found.input.len(),
    );
    fs::write(folder.join(format!("{stem}.txt")), note)?;
    Ok(path)
}

/// Store the minimized input next to the saved failure (`<stem>.min.bin`) and
/// record its size in the note.
fn save_minimized(saved: &Path, reduced: &[u8]) -> io::Result<()> {
    let stem = saved.with_extension("");
    fs::write(stem.with_extension("min.bin"), reduced)?;
    let mut note = fs::read_to_string(stem.with_extension("txt"))?;
    note.push_str(&format!("minimized-bytes: {}\n", reduced.len()));
    fs::write(stem.with_extension("txt"), note)
}

/// Shrink `input` while the same failure (same kind and panic location)
/// keeps reproducing. Removes blocks of decreasing size (half the input, then
/// a quarter, ... down to single bytes) and repeats until a full pass changes
/// nothing, `max_runs` executions were spent, or [`MINIMIZE_MAX_TIME`] passed
/// (a timeout failure costs one full deadline per execution).
pub fn minimize(
    runner: &mut Runner,
    input: &[u8],
    reference: &Failure,
    max_runs: usize,
) -> Vec<u8> {
    let key = reference.key();
    let mut current = input.to_vec();
    let mut runs = 0usize;
    let started = Instant::now();
    let mut reproduces = |candidate: &[u8], runs: &mut usize| {
        *runs += 1;
        matches!(runner.run(candidate), Err(f) if f.key() == key)
    };
    loop {
        let before = current.len();
        let mut block = (current.len() / 2).max(1);
        loop {
            let mut at = 0usize;
            while at < current.len() {
                if runs >= max_runs || started.elapsed() >= MINIMIZE_MAX_TIME {
                    return current;
                }
                let end = (at + block).min(current.len());
                let mut candidate = Vec::with_capacity(current.len() - (end - at));
                candidate.extend_from_slice(&current[..at]);
                candidate.extend_from_slice(&current[end..]);
                if reproduces(&candidate, &mut runs) {
                    current = candidate;
                } else {
                    at += block;
                }
            }
            if block == 1 {
                break;
            }
            block = (block / 2).max(1);
        }
        if current.len() == before {
            return current;
        }
    }
}
