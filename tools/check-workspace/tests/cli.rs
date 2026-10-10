// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str) -> Self {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("atelier-lock-{label}-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, rel: &str, content: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    fn run(&self) -> Output {
        Self::run_at(&self.0)
    }
    fn run_at(root: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_atelier-check-workspace"))
            .arg(root)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn workspace(f: &Fixture, extra_member: &str, lock: &str) {
    f.write(
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"3\"\nmembers = [\"crates/a\"{extra_member}]\n"),
    );
    f.write(
        "crates/a/Cargo.toml",
        "[package]\nname=\"a\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
    );
    f.write("crates/a/src/lib.rs", "");
    f.write("Cargo.lock", lock);
}
#[test]
fn executable_accepts_workspace_and_local_dependency_packages() {
    let f = Fixture::new("local");
    f.write(
        "Cargo.toml",
        "[workspace]\nresolver=\"3\"\nmembers=[\"crates/a\",\"crates/b\"]\n",
    );
    for (name, dep) in [("a", "b = { path = \"../b\" }"), ("b", "")] {
        f.write(&format!("crates/{name}/Cargo.toml"), &format!("[package]\nname=\"{name}\"\nversion=\"1.0.0\"\nedition=\"2024\"\n[dependencies]\n{dep}\n"));
        f.write(&format!("crates/{name}/src/lib.rs"), "");
    }
    f.write("Cargo.lock", "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"b 1.0.0\",\n]\n\n[[package]]\nname = \"b\"\nversion = \"1.0.0\"\n");
    let out = f.run();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn executable_refuses_external_packages_without_changing_lock_bytes() {
    let f = Fixture::new("external");
    let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"outside\"\nversion = \"1.0.0\"\nsource = \"registry+https://example.invalid\"\n";
    workspace(&f, "", lock);
    let before = fs::read(f.0.join("Cargo.lock")).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("external package"));
    assert_eq!(fs::read(f.0.join("Cargo.lock")).unwrap(), before);
}

#[test]
fn executable_refuses_git_source_and_preserves_lock_bytes() {
    let f = Fixture::new("git-source");
    let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"remote\"\nversion = \"1.0.0\"\nsource = \"git+https://example.invalid/repo?rev=main#0123456789abcdef\"\n";
    workspace(&f, "", lock);
    let path = f.0.join("Cargo.lock");
    let before = fs::read(&path).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("external package"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn executable_refuses_registry_source_even_when_name_and_version_match_member() {
    let f = Fixture::new("same-identity");
    let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+https://example.invalid\"\n";
    workspace(&f, "", lock);
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("external package"));
}

#[test]
fn executable_refuses_non_member_path_packages_and_invalid_locks() {
    let f = Fixture::new("nonmember");
    workspace(
        &f,
        "",
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"hidden\"\nversion = \"1.0.0\"\n",
    );
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a workspace member"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for bad in [
        "version = 99\n",
        "version = 4\n[[package]]\nname = \"a\"\n",
        "version = 4\n[[package]]\nname = \"a\"\nname = \"a\"\nversion = \"1.0.0\"\n",
    ] {
        f.write("Cargo.lock", bad);
        let out = f.run();
        assert_eq!(out.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
}

#[test]
fn executable_rejects_malformed_lock_arrays_and_preserves_bytes() {
    let f = Fixture::new("malformed-array");
    workspace(
        &f,
        "",
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n",
    );
    let path = f.0.join("Cargo.lock");
    for lock in [
        "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = []\ndependencies = []\n",
        "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"a\"oops\",\n]\n",
        "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"a\"\n \"b\",\n]\n",
        "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"bad\"value\"\n",
        "version = 4\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"bad\tvalue\"\n",
        "version = 04\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n",
    ] {
        fs::write(&path, lock).unwrap();
        let before = fs::read(&path).unwrap();
        let out = f.run();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn executable_reports_cargo_metadata_failure_without_changing_lock() {
    let f = Fixture::new("metadata-failure");
    workspace(
        &f,
        "",
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n",
    );
    f.write("Cargo.toml", "[workspace]\nmembers = [\n");
    let path = f.0.join("Cargo.lock");
    let before = fs::read(&path).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cargo metadata failed"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn executable_rejects_excluded_path_dependency_without_changing_lock() {
    let f = Fixture::new("excluded-path");
    let root = f.0.join("workspace");
    fs::create_dir_all(&root).unwrap();
    let hidden = f.0.join("hidden");
    fs::create_dir_all(hidden.join("src")).unwrap();
    fs::write(
        hidden.join("Cargo.toml"),
        "[package]\nname=\"hidden\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(hidden.join("src/lib.rs"), "").unwrap();
    let a = root.join("crates/a");
    fs::create_dir_all(a.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver=\"3\"\nmembers=[\"crates/a\"]\n",
    )
    .unwrap();
    fs::write(a.join("Cargo.toml"), "[package]\nname=\"a\"\nversion=\"1.0.0\"\nedition=\"2024\"\n[dependencies]\nhidden={path=\"../../../hidden\"}\n").unwrap();
    fs::write(a.join("src/lib.rs"), "").unwrap();
    let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\ndependencies = [\n \"hidden 1.0.0\",\n]\n\n[[package]]\nname = \"hidden\"\nversion = \"1.0.0\"\n";
    fs::write(root.join("Cargo.lock"), lock).unwrap();
    let path = root.join("Cargo.lock");
    let before = fs::read(&path).unwrap();
    let out = Fixture::run_at(&root);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a workspace member"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn executable_reports_cargo_package_collision_without_changing_lock() {
    let f = Fixture::new("path-collision");
    let root = f.0.join("workspace");
    fs::create_dir_all(root.join("crates/member/src")).unwrap();
    let external = f.0.join("same");
    fs::create_dir_all(external.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver=\"3\"\nmembers=[\"crates/member\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/member/Cargo.toml"),
        "[package]\nname=\"same\"\nversion=\"1.0.0\"\nedition=\"2024\"\n[dependencies]\nsame-external={package=\"same\",path=\"../../../same\"}\n",
    )
    .unwrap();
    fs::write(root.join("crates/member/src/lib.rs"), "").unwrap();
    fs::write(
        external.join("Cargo.toml"),
        "[package]\nname=\"same\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(external.join("src/lib.rs"), "").unwrap();
    let lock = "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\n";
    fs::write(root.join("Cargo.lock"), lock).unwrap();
    let lock_path = root.join("Cargo.lock");
    let before = fs::read(&lock_path).unwrap();
    let out = Fixture::run_at(&root);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cargo metadata failed"), "{stderr}");
    assert!(stderr.to_lowercase().contains("collision"), "{stderr}");
    assert_eq!(fs::read(lock_path).unwrap(), before);
}

#[test]
fn executable_refuses_lock_that_needs_resolution_without_changing_bytes() {
    let f = Fixture::new("needs-resolution");
    f.write(
        "Cargo.toml",
        "[workspace]\nresolver=\"3\"\nmembers=[\"crates/a\",\"crates/b\"]\n",
    );
    for name in ["a", "b"] {
        let dep = if name == "a" {
            "[dependencies]\nb = { path = \"../b\" }\n"
        } else {
            ""
        };
        f.write(
            &format!("crates/{name}/Cargo.toml"),
            &format!("[package]\nname=\"{name}\"\nversion=\"1.0.0\"\nedition=\"2024\"\n{dep}"),
        );
        f.write(&format!("crates/{name}/src/lib.rs"), "");
    }
    let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n";
    f.write("Cargo.lock", lock);
    let path = f.0.join("Cargo.lock");
    let before = fs::read(&path).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cargo metadata failed"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn executable_reports_missing_and_invalid_utf8_lock() {
    let f = Fixture::new("missing");
    f.write("Cargo.toml", "[workspace]\nmembers=[]\n");
    assert_eq!(f.run().status.code(), Some(1));
    fs::write(f.0.join("Cargo.lock"), [0xff, 0xfe]).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("UTF-8"));
}

#[test]
fn executable_rejects_extra_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_atelier-check-workspace"))
        .args([Path::new("."), Path::new("extra")])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
