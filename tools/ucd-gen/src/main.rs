// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::PathBuf;

fn usage() -> &'static str {
    "usage: atelier-ucd-gen --ucd DIR --output FILE [--check]"
}
fn main() {
    let mut args = std::env::args_os().skip(1);
    let mut ucd = None;
    let mut output = None;
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--ucd") => {
                if ucd.is_some() {
                    eprintln!("duplicate --ucd\n{}", usage());
                    std::process::exit(2);
                }
                ucd = args.next().map(PathBuf::from);
                if ucd.is_none() {
                    eprintln!("missing --ucd value\n{}", usage());
                    std::process::exit(2);
                }
            }
            Some("--output") => {
                if output.is_some() {
                    eprintln!("duplicate --output\n{}", usage());
                    std::process::exit(2);
                }
                output = args.next().map(PathBuf::from);
                if output.is_none() {
                    eprintln!("missing --output value\n{}", usage());
                    std::process::exit(2);
                }
            }
            Some("--check") => {
                if check {
                    eprintln!("duplicate --check\n{}", usage());
                    std::process::exit(2);
                }
                check = true;
            }
            _ => {
                eprintln!("unknown argument\n{}", usage());
                std::process::exit(2);
            }
        }
    }
    let (Some(ucd), Some(output)) = (ucd, output) else {
        eprintln!("{}", usage());
        std::process::exit(2)
    };
    if let Err(e) = atelier_ucd_gen::run(&ucd, &output, check) {
        eprintln!("atelier-ucd-gen: {e}");
        std::process::exit(1);
    }
}
