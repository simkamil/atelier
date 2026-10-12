// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_image_diff::{
    Config, DiffError, Tolerance, UPDATE_ENV, check_image_matches, compare, diff_image, load_png,
    save_png,
};
use std::env;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage:\n  atelier-image-diff compare <a.png> <b.png> [--channel-tolerance N] [--max-different PCT] [--diff out.png]\n  atelier-image-diff update <actual.png> <reference.png>\n\
compare: exit 0 when the images are identical within the tolerance, 1 when they differ,\n\
2 on a usage or I/O error. N is 0-255 (default 0), PCT is 0-100 (default 0).\n\
update: overwrites the reference with the actual image (refused when CI is set).";

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
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        Some("compare") => run_compare(&args[1..]),
        Some("update") => run_update(&args[1..]),
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(0)
        }
        _ => Err("expected a command: compare or update".into()),
    }
}

fn run_compare(args: &[String]) -> Result<u8, String> {
    let mut files = Vec::new();
    let mut channel = 0u8;
    let mut percent = 0.0f64;
    let mut diff_path = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "--channel-tolerance" => {
                channel = value(arg)?
                    .parse()
                    .map_err(|_| "--channel-tolerance must be an integer from 0 to 255")?;
            }
            "--max-different" => {
                percent = value(arg)?
                    .parse()
                    .map_err(|_| "--max-different must be a number from 0 to 100")?;
            }
            "--diff" => diff_path = Some(value(arg)?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            _ => files.push(arg.clone()),
        }
    }
    let [a, b] = files.as_slice() else {
        return Err("compare takes exactly two images".into());
    };
    let tolerance = Tolerance::new(channel, percent);
    let a = load_png(Path::new(a)).map_err(|e| e.to_string())?;
    let b = load_png(Path::new(b)).map_err(|e| e.to_string())?;
    let report = match compare(&a, &b, &tolerance) {
        Ok(report) => report,
        Err(error @ DiffError::DimensionMismatch { .. }) => {
            println!("{error}\nresult: different");
            return Ok(1);
        }
        Err(error) => return Err(error.to_string()),
    };
    println!("{report}");
    if let Some(path) = diff_path {
        let image = diff_image(&a, &b, &tolerance).map_err(|e| e.to_string())?;
        save_png(Path::new(&path), &image).map_err(|e| e.to_string())?;
        println!("difference image: {path}");
    }
    Ok(if report.passed { 0 } else { 1 })
}

fn run_update(args: &[String]) -> Result<u8, String> {
    let [actual, reference] = args else {
        return Err("update takes <actual.png> <reference.png>".into());
    };
    let config = Config {
        update: true,
        ..Config::from_env()
    };
    let image = load_png(Path::new(actual)).map_err(|e| e.to_string())?;
    let outcome = check_image_matches(&image, Path::new(reference), &Tolerance::exact(), &config)
        .map_err(|e| e.to_string())?;
    println!("{reference}: {outcome:?} ({UPDATE_ENV} not needed with this command)");
    Ok(0)
}
