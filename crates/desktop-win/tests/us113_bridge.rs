// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : test d'intégration natif du pont (SharedBuffer, tickets, écriture atomique, polices) par le
//! mode diagnostic `--bridge-self-check`, sans dialogue interactif. Il lit réellement un fichier de
//! 100 Mio dans la vraie page d'une WebView2 masquée et applique la cible de 300 ms à la médiane.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn shared_buffer_reads_100_mib_under_300_ms_and_releases_every_buffer() {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("us113-bridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let output = Command::new(env!("CARGO_BIN_EXE_atelier"))
        .arg("--bridge-self-check")
        .arg(&directory)
        .output()
        .expect("launch atelier.exe");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_dir_all(&directory);
    if stderr.contains("runtime is unavailable") || stderr.contains("WebView2 Evergreen runtime") {
        eprintln!("SKIPPED: WebView2 Evergreen runtime is not installed; nothing was measured");
        return;
    }
    assert!(
        output.status.success(),
        "bridge self-check failed: {stdout}{stderr}"
    );
    println!("{stdout}");
    assert!(
        stdout.contains("bridge: shared-buffer read of 100 MiB x7"),
        "{stdout}"
    );
    let number = |key: &str| -> f64 {
        let start = stdout
            .find(key)
            .unwrap_or_else(|| panic!("{key} missing in {stdout}"))
            + key.len();
        let end = stdout[start..]
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map_or(stdout.len(), |at| start + at);
        stdout[start..end].parse().unwrap()
    };
    let median = number("median=");
    assert!(number("min=") <= median && median <= number("max="));
    assert!(
        median < 300.0,
        "median {median} ms must be under the 300 ms target: {stdout}"
    );
    assert_eq!(
        number("received="),
        number("released="),
        "every shared buffer must be released: {stdout}"
    );
    assert!(number("posted=") >= 7.0, "{stdout}");
    assert!(stdout.contains("spent/unknown tickets refused"), "{stdout}");
    assert!(
        stdout.contains("callback objects 0->0"),
        "no COM callback object may leak: {stdout}"
    );
}
