// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{env, fs, path::PathBuf};

fn watch(path: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap_or_else(|error| panic!("cannot scan UCD directory {}: {error}", path.display()))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("cannot scan UCD directory: {error}"))
                    .path()
            })
            .collect();
        entries.sort();
        for entry in entries {
            watch(&entry);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ucd = manifest.join("../../data/ucd/17.0.0");
    let checked_in = manifest.join("src/generated.rs");
    watch(&ucd);
    println!("cargo:rerun-if-changed={}", checked_in.display());
    let generated = atelier_ucd_gen::generate(&ucd)
        .unwrap_or_else(|error| panic!("cannot generate Unicode 17.0.0 tables: {error}"));
    let committed = fs::read(&checked_in)
        .unwrap_or_else(|error| panic!("cannot read checked-in UCD tables: {error}"));
    assert!(
        generated == committed,
        "generated UCD tables differ from src/generated.rs; regenerate with atelier-ucd-gen"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("generated.rs"), generated)
        .unwrap_or_else(|error| panic!("cannot write generated UCD tables to OUT_DIR: {error}"));
}
