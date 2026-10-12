// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_fuzz::campaign::{DEFAULT_MINIMIZE_RUNS, fingerprint};
use atelier_fuzz::{Config, FuzzTarget, Runner, find, fuzz, minimize, registry};
use std::env;
use std::fs;
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
  --inflight        rewrite crashes/inflight.bin before every execution (survives aborts)\n\
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
            "--max-len" => options.max_len = parse_number(&value("--max-len")?)? as usize,
            "--max-output-mib" => {
                options.max_output_mib = parse_number(&value("--max-output-mib")?)?
            }
            "--duration" => {
                let text = value("--duration")?;
                let seconds: f64 = text
                    .parse()
                    .map_err(|_| format!("'{text}' is not a valid duration"))?;
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err(format!("'{text}' is not a valid duration"));
                }
                options.duration = Some(Duration::from_secs_f64(seconds));
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
    let mut runner = Runner::new(
        target_named(name)?,
        options.timeout,
        options.max_output_mib * 1024 * 1024,
    );
    let Err(failure) = runner.run(&input) else {
        return Err(format!("'{file}' does not make '{name}' fail"));
    };
    let reduced = minimize(&mut runner, &input, &failure, DEFAULT_MINIMIZE_RUNS);
    let output = options
        .output
        .clone()
        .unwrap_or_else(|| format!("{file}.min"));
    fs::write(&output, &reduced).map_err(|e| format!("cannot write '{output}': {e}"))?;
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
