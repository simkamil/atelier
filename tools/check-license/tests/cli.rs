// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn missing_source_roots_print_diagnostics_and_exit_nonzero() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("atelier-license-cli-{unique}"));
    let result = Command::new(env!("CARGO_BIN_EXE_atelier-check-license"))
        .arg(&root)
        .output()
        .expect("run checker");

    assert!(!result.status.success(), "missing source roots must fail");
    let stderr = String::from_utf8(result.stderr).expect("diagnostic is UTF-8");
    for source_root in ["crates", "tools"] {
        let path = PathBuf::from(&root).join(source_root);
        assert!(
            stderr.contains(&path.display().to_string()),
            "missing path diagnostic: {stderr}"
        );
        assert!(
            stderr.contains("missing source root"),
            "missing root diagnostic: {stderr}"
        );
    }
    let _ = fs::remove_dir_all(root);
}
