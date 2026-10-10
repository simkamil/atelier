// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_bench::{MAX_ITERATIONS, compare, default_registry, parse_csv, to_csv};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage: atelier-bench [--list] [--case NAME]... [--iterations N] [--output FILE] [--reference FILE]\n\
Measures selected cases (all by default) and writes CSV with median nanoseconds per iteration.\n\
Run with `cargo run --release -p atelier-bench --offline -- --iterations 100 --output current.csv`.\n\
Pass --reference baseline.csv to compare; exit 2 indicates a regression (>10%), exit 1 an error.";

#[derive(Debug)]
struct Options {
    help: bool,
    list: bool,
    cases: Vec<String>,
    iterations: usize,
    output: String,
    reference: Option<String>,
}

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

fn run(args: Vec<String>) -> Result<u8, String> {
    let options = parse_args(&args)?;
    let mut registry = default_registry();
    if options.help {
        println!("{USAGE}");
        return Ok(0);
    }
    if options.list {
        for name in registry.names() {
            println!("{name}");
        }
        return Ok(0);
    }
    let reference = if let Some(path) = options.reference.as_deref() {
        let contents =
            fs::read_to_string(path).map_err(|e| format!("cannot read reference '{path}': {e}"))?;
        let rows = parse_csv(&contents).map_err(|e| format!("invalid reference '{path}': {e}"))?;
        reject_same_file(&options.output, path)?;
        Some(rows)
    } else {
        None
    };

    let results = registry.measure(&options.cases, options.iterations)?;
    let regressions = reference
        .as_ref()
        .map(|rows| compare(&results, rows))
        .transpose()?;
    let csv = to_csv(&results);
    if options.output == "-" {
        print!("{csv}");
    } else {
        fs::write(&options.output, csv)
            .map_err(|e| format!("cannot write '{}': {e}", options.output))?;
    }
    for result in &results {
        eprintln!(
            "{}: {} iterations, median {} ns",
            result.case, result.iterations, result.median_ns
        );
    }
    if let Some(regressions) = regressions {
        if regressions.is_empty() {
            eprintln!("comparison: no regression above 10%");
        } else {
            for regression in regressions {
                eprintln!(
                    "regression: {}: {} ns vs {} ns reference (>10%)",
                    regression.case, regression.measured_ns, regression.reference_ns
                );
            }
            return Ok(2);
        }
    }
    Ok(0)
}

fn reject_same_file(output: &str, reference: &str) -> Result<(), String> {
    if output == "-" {
        return Ok(());
    }
    let output_path = Path::new(output);
    let reference_path = Path::new(reference);
    if let (Ok(output_canonical), Ok(reference_canonical)) = (
        fs::canonicalize(output_path),
        fs::canonicalize(reference_path),
    ) {
        if output_canonical == reference_canonical {
            return Err("output and reference resolve to the same file".into());
        }
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        help: false,
        list: false,
        cases: Vec::new(),
        iterations: 20,
        output: "bench.csv".into(),
        reference: None,
    };
    let mut i = 0;
    let mut measurement_option_seen = false;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => options.help = true,
            "--list" => options.list = true,
            "--case" => {
                measurement_option_seen = true;
                i += 1;
                options
                    .cases
                    .push(args.get(i).ok_or("--case requires a name")?.clone());
            }
            "--iterations" => {
                measurement_option_seen = true;
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or("--iterations requires a positive integer")?;
                options.iterations = raw
                    .parse()
                    .map_err(|_| format!("invalid iteration count '{raw}'"))?;
                if options.iterations == 0 {
                    return Err("iteration count must be greater than zero".into());
                }
                if options.iterations > MAX_ITERATIONS {
                    return Err(format!(
                        "iteration count exceeds the maximum of {MAX_ITERATIONS}"
                    ));
                }
            }
            "--output" => {
                measurement_option_seen = true;
                i += 1;
                options.output = args
                    .get(i)
                    .ok_or("--output requires a file path (or - for stdout)")?
                    .clone();
            }
            "--reference" => {
                measurement_option_seen = true;
                i += 1;
                options.reference = Some(
                    args.get(i)
                        .ok_or("--reference requires a CSV path")?
                        .clone(),
                );
            }
            unknown => return Err(format!("unknown argument '{unknown}'")),
        }
        i += 1;
    }
    if options.list && measurement_option_seen {
        return Err("--list cannot be combined with measurement options".into());
    }
    if options.help && (options.list || !options.cases.is_empty() || options.reference.is_some()) {
        return Err("--help cannot be combined with other options".into());
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(label: &str) -> String {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("atelier-bench-{label}-{suffix}.csv"))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn cli_rejects_invalid_arguments_and_unreadable_reference() {
        assert!(
            run(vec!["--wat".into()])
                .unwrap_err()
                .contains("unknown argument")
        );
        assert!(
            run(vec!["--iterations".into(), "0".into()])
                .unwrap_err()
                .contains("greater than zero")
        );
        assert!(
            run(vec!["--iterations".into(), "many".into()])
                .unwrap_err()
                .contains("invalid iteration")
        );
        assert!(
            run(vec!["--reference".into(), "missing-file.csv".into()])
                .unwrap_err()
                .contains("cannot read reference")
        );
        assert_eq!(run(vec!["--help".into()]).unwrap(), 0);
        assert!(
            run(vec!["--case".into(), "missing".into()])
                .unwrap_err()
                .contains("unknown benchmark")
        );
    }

    #[test]
    fn cli_reports_unwritable_output_and_incomplete_reference() {
        let output = std::env::temp_dir()
            .join(format!("atelier-bench-no-dir-{}", std::process::id()))
            .join("out.csv")
            .to_string_lossy()
            .into_owned();
        let error = run(vec!["--output".into(), output.clone()]).unwrap_err();
        assert!(error.contains("cannot write"));

        let reference = temp_file("incomplete");
        fs::write(&reference, "case,iterations,median_ns\nutf8_scan,1,10\n").unwrap();
        let error = run(vec!["--reference".into(), reference.clone()]).unwrap_err();
        assert!(error.contains("missing benchmark case"));
        let _ = fs::remove_file(reference);
    }

    #[test]
    fn cli_preserves_reference_and_does_not_create_output_for_invalid_reference() {
        let baseline = temp_file("same-file");
        let baseline_contents = "case,iterations,median_ns\nutf8_scan,1,100000000000000000000000000000000000000\nsort_u64,1,100000000000000000000000000000000000000\n";
        fs::write(&baseline, baseline_contents).unwrap();
        let error = run(vec![
            "--output".into(),
            baseline.clone(),
            "--reference".into(),
            baseline.clone(),
        ])
        .unwrap_err();
        assert!(error.contains("same file"));
        assert_eq!(fs::read_to_string(&baseline).unwrap(), baseline_contents);

        let invalid_reference = temp_file("bad-reference");
        let output = temp_file("must-not-exist");
        fs::write(&invalid_reference, "malformed\n").unwrap();
        let error = run(vec![
            "--output".into(),
            output.clone(),
            "--reference".into(),
            invalid_reference.clone(),
        ])
        .unwrap_err();
        assert!(error.contains("invalid reference"));
        assert!(!Path::new(&output).exists());
        let _ = fs::remove_file(baseline);
        let _ = fs::remove_file(invalid_reference);
    }

    #[test]
    fn cli_lists_and_measures_multiple_real_cases() {
        assert_eq!(run(vec!["--list".into()]).unwrap(), 0);
        let path = temp_file("multi");
        assert_eq!(
            run(vec![
                "--iterations".into(),
                "2".into(),
                "--output".into(),
                path.clone()
            ])
            .unwrap(),
            0
        );
        let csv = fs::read_to_string(&path).unwrap();
        let parsed = parse_csv(&csv).unwrap();
        assert_eq!(parsed.len(), 2);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn cli_rejects_list_measurement_option_combinations() {
        assert!(
            parse_args(&["--list".into(), "--iterations".into(), "20".into()])
                .unwrap_err()
                .contains("cannot be combined")
        );
    }

    #[test]
    fn cli_refuses_malformed_reference_without_panicking() {
        let baseline = temp_file("malformed");
        fs::write(&baseline, "not,csv\n").unwrap();
        let error = run(vec!["--reference".into(), baseline.clone()]).unwrap_err();
        assert!(error.contains("invalid reference"));
        let _ = fs::remove_file(baseline);
    }
}
