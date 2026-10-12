// SPDX-License-Identifier: AGPL-3.0-or-later
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command, Output};

fn fuzz(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atelier-fuzz"))
        .args(args)
        .output()
        .expect("run atelier-fuzz")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-fuzz-cli-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn lists_targets() {
    let out = fuzz(&["list"]);
    assert_eq!(out.status.code(), Some(0));
    let listing = text(&out.stdout);
    assert!(listing.lines().any(|l| l == "inflate"));
    assert!(listing.lines().any(|l| l == "recovery-journal"));
}

#[test]
fn runs_a_target_and_prints_the_summary() {
    let dir = root("run");
    let out = fuzz(&[
        "run",
        "xml",
        "--iterations",
        "200",
        "--seed",
        "0x10",
        "--root",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let summary = text(&out.stdout);
    assert!(
        summary.contains("xml: seed 16, 200 iterations"),
        "{summary}"
    );
    assert!(
        summary.contains("corpus") && summary.contains("failures 0"),
        "{summary}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn runs_for_a_duration_and_writes_and_replays_seeds() {
    let dir = root("seed");
    let root_arg = dir.to_str().unwrap();
    assert_eq!(
        fuzz(&["seed", "png", "--root", root_arg]).status.code(),
        Some(0)
    );
    let seed = fs::read_dir(dir.join("corpus").join("png"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let replay = fuzz(&["replay", "png", seed.to_str().unwrap()]);
    assert_eq!(replay.status.code(), Some(0));
    assert!(text(&replay.stdout).contains(": ok"));
    let timed = fuzz(&["run", "png", "--duration", "0.2", "--root", root_arg]);
    assert_eq!(timed.status.code(), Some(0));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_bad_usage_with_exit_code_one() {
    for args in [
        &["run", "nope"][..],
        &["run"],
        &["run", "xml", "--seed"],
        &["run", "xml", "--seed", "abc"],
        &["run", "xml", "--bogus"],
        &["replay", "xml"],
        &["minimize", "xml", "/definitely/missing"],
        &["frobnicate"],
        &[],
    ] {
        let out = fuzz(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(text(&out.stderr).contains("Usage:"), "{args:?}");
    }
}

#[test]
fn minimize_refuses_an_input_that_does_not_fail() {
    let dir = root("min");
    let file = dir.join("ok.bin");
    fs::write(&file, b"<a/>").unwrap();
    let out = fuzz(&["minimize", "xml", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("does not make"));
    let _ = fs::remove_dir_all(dir);
}
