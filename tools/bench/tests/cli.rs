// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use atelier_bench::parse_csv;
use std::fs;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atelier-bench"))
        .args(args)
        .output()
        .expect("run benchmark CLI")
}

fn temp_file(label: &str) -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("atelier-bench-cli-{label}-{suffix}.csv"))
}

fn stdout_csv(output: &Output) -> Vec<atelier_bench::BenchmarkResult> {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_csv(std::str::from_utf8(&output.stdout).expect("stdout is UTF-8")).expect("stdout is CSV")
}

#[test]
fn executable_stdout_is_csv_with_and_without_reference() {
    let without_reference = run(&["--iterations", "2", "--output", "-"]);
    assert_eq!(stdout_csv(&without_reference).len(), 2);

    let reference_path = temp_file("baseline");
    fs::write(
        &reference_path,
        "case,iterations,median_ns\nutf8_scan,1,340282366920938463463374607431768211455\nsort_u64,1,340282366920938463463374607431768211455\n",
    ).expect("write baseline");
    let reference_arg = reference_path.to_string_lossy().into_owned();
    let with_reference = Command::new(env!("CARGO_BIN_EXE_atelier-bench"))
        .args([
            "--iterations",
            "2",
            "--output",
            "-",
            "--reference",
            &reference_arg,
        ])
        .output()
        .expect("run comparison");
    assert_eq!(stdout_csv(&with_reference).len(), 2);
    assert!(String::from_utf8_lossy(&with_reference.stderr).contains("comparison: no regression"));
    let _ = fs::remove_file(reference_path);
}

#[test]
fn executable_list_and_case_selection_return_only_requested_rows() {
    let listing = run(&["--list"]);
    assert!(listing.status.success());
    let names = String::from_utf8(listing.stdout).expect("list output is UTF-8");
    assert_eq!(names.lines().collect::<Vec<_>>(), ["utf8_scan", "sort_u64"]);

    let selected = run(&["--case", "utf8_scan", "--iterations", "2", "--output", "-"]);
    let rows = stdout_csv(&selected);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].case, "utf8_scan");
}

#[test]
fn executable_rejects_same_output_and_reference_without_changing_baseline() {
    let baseline = temp_file("same-file");
    let contents = "case,iterations,median_ns\nutf8_scan,1,100\nsort_u64,1,100\n";
    fs::write(&baseline, contents).expect("write baseline");
    let path = baseline.to_string_lossy().into_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_atelier-bench"))
        .args(["--output", &path, "--reference", &path])
        .output()
        .expect("run CLI");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("same file"));
    assert_eq!(fs::read_to_string(&baseline).unwrap(), contents);
    let _ = fs::remove_file(baseline);
}

#[test]
fn executable_rejects_extreme_iteration_counts_without_panicking() {
    let extreme = usize::MAX.to_string();
    let output = run(&["--iterations", &extreme, "--output", "-"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("maximum"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn executable_reports_non_unicode_os_argument() {
    use std::os::unix::ffi::OsStringExt;
    let output = Command::new(env!("CARGO_BIN_EXE_atelier-bench"))
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .output()
        .expect("run CLI");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not valid Unicode"));
}

#[cfg(windows)]
#[test]
fn executable_reports_non_unicode_os_argument() {
    use std::os::windows::ffi::OsStringExt;
    let output = Command::new(env!("CARGO_BIN_EXE_atelier-bench"))
        .arg(std::ffi::OsString::from_wide(&[0xd800]))
        .output()
        .expect("run CLI");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not valid Unicode"));
}
