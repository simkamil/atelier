// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_fuzz::campaign::{
    DEFAULT_MINIMIZE_RUNS, fingerprint, minimize_with, save_minimization_timeout,
};
use atelier_fuzz::{Config, FuzzTarget, Runner, find, fuzz, registry};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = "Usage: atelier-fuzz <command> [options]\n\
Commands:\n\
  list                              print the target names\n\
  run <TARGET|--all> [options]      fuzz one target, or every target in turn\n\
  replay <TARGET> <FILE>...         run saved inputs once\n\
  minimize <TARGET> <FILE> [--output FILE]\n\
                                    shrink a failing input (default output: FILE.min)\n\
  seed [TARGET]                     write the built-in seeds to corpus/<target>/\n\
Options for run:\n\
  --seed N          generator seed, decimal or 0x hex (default 1)\n\
  --duration SECS   stop each target after SECS seconds (fractions allowed)\n\
  --iterations N    stop each target after N executions (default 10000 without --duration)\n\
  --timeout-ms N    per-input deadline (default 5000); a hang saves the input and exits 3\n\
  --max-len BYTES   largest mutated input (default 65536)\n\
  --max-output-mib N  memory budget checked against the output size the target reports (default 256)\n\
  --minimize        also store a minimized copy of each new failure\n\
  --inflight        rewrite crashes/inflight.bin before every campaign/minimize execution\n\
Common options: --root DIR (default: the tools/fuzz directory), --timeout-ms for replay and minimize.\n\
Exit codes: 0 no failure, 1 usage or I/O error, 2 failure found, 3 hang found.";

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string().map_err(|arg| {
                format!("argument is not valid Unicode: '{}'", arg.to_string_lossy())
            })
        })
        .collect::<Result<Vec<_>, _>>();
    match args.and_then(run) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug)]
struct Options {
    positional: Vec<String>,
    all: bool,
    seed: u64,
    duration: Option<Duration>,
    iterations: Option<u64>,
    timeout: Duration,
    max_len: usize,
    max_output_mib: u64,
    minimize: bool,
    inflight: bool,
    output: Option<String>,
    root: PathBuf,
}

fn parse_number(text: &str) -> Result<u64, String> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("'{text}' is not a valid number"))
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        positional: Vec::new(),
        all: false,
        seed: 1,
        duration: None,
        iterations: None,
        timeout: Duration::from_millis(5000),
        max_len: 64 * 1024,
        max_output_mib: 256,
        minimize: false,
        inflight: false,
        output: None,
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--all" => options.all = true,
            "--minimize" => options.minimize = true,
            "--inflight" => options.inflight = true,
            "--seed" => options.seed = parse_number(&value("--seed")?)?,
            "--iterations" => options.iterations = Some(parse_number(&value("--iterations")?)?),
            "--timeout-ms" => {
                options.timeout = Duration::from_millis(parse_number(&value("--timeout-ms")?)?)
            }
            "--max-len" => {
                let value = parse_number(&value("--max-len")?)?;
                options.max_len = usize::try_from(value)
                    .map_err(|_| "--max-len does not fit this platform".to_string())?;
            }
            "--max-output-mib" => {
                options.max_output_mib = parse_number(&value("--max-output-mib")?)?;
                options
                    .max_output_mib
                    .checked_mul(1024 * 1024)
                    .ok_or_else(|| "--max-output-mib is too large".to_string())?;
            }
            "--duration" => {
                let text = value("--duration")?;
                let seconds: f64 = text
                    .parse()
                    .map_err(|_| format!("'{text}' is not a valid duration"))?;
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err(format!("'{text}' is not a valid duration"));
                }
                options.duration = Some(
                    Duration::try_from_secs_f64(seconds)
                        .map_err(|_| format!("'{text}' is too large for a duration"))?,
                );
            }
            "--output" => options.output = Some(value("--output")?),
            "--root" => options.root = PathBuf::from(value("--root")?),
            flag if flag.starts_with("--") => return Err(format!("unknown option '{flag}'")),
            _ => options.positional.push(arg.clone()),
        }
    }
    if options.max_len == 0 {
        return Err("--max-len must be positive".to_string());
    }
    Ok(options)
}

fn target_named(name: &str) -> Result<Arc<dyn FuzzTarget>, String> {
    find(name).ok_or_else(|| format!("unknown target '{name}' (see `list`)"))
}

fn run(args: Vec<String>) -> Result<u8, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err("missing command".to_string());
    };
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let options = parse_args(rest)?;
    match command.as_str() {
        "list" => {
            for target in registry() {
                println!("{}", target.name());
            }
            Ok(0)
        }
        "run" => run_campaigns(&options),
        "replay" => replay(&options),
        "minimize" => minimize_file(&options),
        "seed" => write_seeds(&options),
        other => Err(format!("unknown command '{other}'")),
    }
}

fn run_campaigns(options: &Options) -> Result<u8, String> {
    let targets: Vec<Arc<dyn FuzzTarget>> = if options.all {
        registry()
    } else {
        match options.positional.as_slice() {
            [name] => vec![target_named(name)?],
            _ => return Err("run needs exactly one TARGET, or --all".to_string()),
        }
    };
    let iterations = match (options.iterations, options.duration) {
        (None, None) => Some(10_000),
        (iterations, _) => iterations,
    };
    let crashes = options.root.join("crashes");
    fs::create_dir_all(&crashes)
        .map_err(|e| format!("cannot create {}: {e}", crashes.display()))?;
    let mut exit = 0u8;
    let (mut total_iterations, mut total_failures) = (0u64, 0usize);
    for target in targets {
        let config = Config {
            seed: options.seed,
            iterations,
            duration: options.duration,
            timeout: options.timeout,
            max_len: options.max_len,
            max_output: options.max_output_mib * 1024 * 1024,
            corpus_dir: Some(options.root.join("corpus").join(target.name())),
            crashes_dir: Some(crashes.clone()),
            minimize: options.minimize,
            inflight: options.inflight.then(|| crashes.join("inflight.bin")),
        };
        let summary = fuzz(target.clone(), &config).map_err(|e| format!("I/O error: {e}"))?;
        println!(
            "{}: seed {}, {} iterations in {:.2} s ({:.0}/s), corpus {}, failures {}{}",
            summary.target,
            summary.seed,
            summary.iterations,
            summary.elapsed.as_secs_f64(),
            summary.per_second(),
            summary.corpus_size,
            summary.failures.len(),
            if summary.duplicate_failures > 0 {
                format!(" (+{} duplicates)", summary.duplicate_failures)
            } else {
                String::new()
            },
        );
        for found in &summary.failures {
            println!(
                "  FAIL {} (iteration {}, {} bytes{}) saved: {}",
                found.failure.describe(),
                found.iteration,
                found.input.len(),
                found
                    .minimized
                    .as_ref()
                    .map(|m| format!(", minimized {} bytes", m.len()))
                    .unwrap_or_default(),
                found
                    .saved
                    .as_ref()
                    .map_or("-".to_string(), |p| p.display().to_string()),
            );
        }
        total_iterations += summary.iterations;
        total_failures += summary.failures.len();
        if summary.hung {
            eprintln!("hang detected in '{}': stopping", summary.target);
            return Ok(3);
        }
        if !summary.failures.is_empty() {
            exit = 2;
        }
    }
    if options.all {
        println!("total: {total_iterations} iterations, {total_failures} failures");
    }
    Ok(exit)
}

fn read_input(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read '{path}': {e}"))
}

fn replay(options: &Options) -> Result<u8, String> {
    let Some((name, files)) = options.positional.split_first() else {
        return Err("replay needs a TARGET and at least one FILE".to_string());
    };
    if files.is_empty() {
        return Err("replay needs at least one FILE".to_string());
    }
    let mut runner = Runner::new(
        target_named(name)?,
        options.timeout,
        options.max_output_mib * 1024 * 1024,
    );
    let mut exit = 0u8;
    for file in files {
        match runner.run(&read_input(file)?) {
            Ok(outcome) => println!(
                "{file}: ok (class {:016x}, output {} bytes)",
                outcome.class, outcome.output_size
            ),
            Err(failure) => {
                println!("{file}: FAIL {}", failure.describe());
                exit = exit.max(2);
                if failure.kind == atelier_fuzz::FailureKind::Timeout {
                    exit = 3;
                    break;
                }
            }
        }
    }
    Ok(exit)
}

fn minimize_file(options: &Options) -> Result<u8, String> {
    let [name, file] = options.positional.as_slice() else {
        return Err("minimize needs a TARGET and one FILE".to_string());
    };
    let input = read_input(file)?;
    let crashes = options.root.join("crashes");
    fs::create_dir_all(&crashes)
        .map_err(|e| format!("cannot create {}: {e}", crashes.display()))?;
    let inflight = options.inflight.then(|| crashes.join("inflight.bin"));
    if let Some(path) = &inflight {
        fs::write(path, &input).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    let target = target_named(name)?;
    let mut runner = Runner::new(
        target.clone(),
        options.timeout,
        options.max_output_mib * 1024 * 1024,
    );
    let Err(failure) = runner.run(&input) else {
        return Err(format!("'{file}' does not make '{name}' fail"));
    };
    if failure.kind == atelier_fuzz::FailureKind::Timeout {
        let saved =
            save_minimization_timeout(&crashes, target.name(), options.seed, 1, input, failure)
                .map_err(|e| format!("cannot save timeout: {e}"))?;
        eprintln!(
            "minimize timed out: the worker thread cannot be stopped; saved {}",
            saved.display()
        );
        return Ok(3);
    }
    let minimized = minimize_with(
        &mut runner,
        &input,
        &failure,
        DEFAULT_MINIMIZE_RUNS,
        |candidate| {
            if let Some(path) = &inflight {
                fs::write(path, candidate)?;
            }
            Ok(())
        },
    )
    .map_err(|e| format!("cannot minimize input: {e}"))?;
    if let Some((candidate, timeout)) = minimized.timeout {
        let saved =
            save_minimization_timeout(&crashes, target.name(), options.seed, 1, candidate, timeout)
                .map_err(|e| format!("cannot save timeout candidate: {e}"))?;
        eprintln!(
            "minimization candidate timed out; stopping without a reduced output (saved {})",
            saved.display()
        );
        return Ok(3);
    }
    let reduced = minimized.reduced;
    let output = options
        .output
        .clone()
        .unwrap_or_else(|| format!("{file}.min"));
    write_new_output(file, &output, &reduced)?;
    println!(
        "{}: {} -> {} bytes ({}), written to {output} (fingerprint {})",
        failure.describe(),
        input.len(),
        reduced.len(),
        failure.key(),
        fingerprint(&reduced),
    );
    Ok(0)
}

fn write_new_output(input: &str, output: &str, bytes: &[u8]) -> Result<(), String> {
    let input_path =
        fs::canonicalize(input).map_err(|e| format!("cannot resolve input '{input}': {e}"))?;
    if let Ok(output_path) = fs::canonicalize(output)
        && output_path == input_path
    {
        return Err("minimized output must not be the input file".to_string());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| format!("cannot create new output '{output}': {e}"))?;
    if let Err(error) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(output);
        return Err(format!("cannot write '{output}': {error}"));
    }
    Ok(())
}

fn write_seeds(options: &Options) -> Result<u8, String> {
    let targets = match options.positional.as_slice() {
        [] => registry(),
        [name] => vec![target_named(name)?],
        _ => return Err("seed takes at most one TARGET".to_string()),
    };
    for target in targets {
        let dir = options.root.join("corpus").join(target.name());
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let seeds = target.seeds();
        for seed in &seeds {
            let path = dir.join(format!("seed-{}.bin", fingerprint(seed)));
            fs::write(&path, seed).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        println!("{}: {} seeds", target.name(), seeds.len());
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::write_new_output;
    use std::fs;
    use std::process;

    #[test]
    fn minimized_output_never_overwrites_the_input_or_existing_files() {
        let dir = std::env::temp_dir().join(format!("atelier-fuzz-output-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.bin");
        let existing = dir.join("existing.bin");
        let new = dir.join("new.bin");
        fs::write(&input, b"original").unwrap();
        fs::write(&existing, b"keep").unwrap();

        assert!(
            write_new_output(input.to_str().unwrap(), input.to_str().unwrap(), b"lost").is_err()
        );
        assert!(
            write_new_output(input.to_str().unwrap(), existing.to_str().unwrap(), b"lost").is_err()
        );
        assert_eq!(fs::read(&input).unwrap(), b"original");
        assert_eq!(fs::read(&existing).unwrap(), b"keep");

        write_new_output(input.to_str().unwrap(), new.to_str().unwrap(), b"reduced").unwrap();
        assert_eq!(fs::read(&new).unwrap(), b"reduced");
        let _ = fs::remove_dir_all(dir);
    }
}
