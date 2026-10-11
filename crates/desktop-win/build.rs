// SPDX-License-Identifier: AGPL-3.0-or-later
//! Link Microsoft's permitted system-API loader into the portable executable.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../../data/webview2/manifest.json");
    println!("cargo:rerun-if-changed=../../data/webview2/x64/WebView2LoaderStatic.lib");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert!(
        std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
            && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc"),
        "The verified WebView2 loader supports x86_64-pc-windows-msvc only"
    );
    let directory = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../data/webview2/x64");
    assert!(directory.join("WebView2LoaderStatic.lib").is_file());
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=WebView2LoaderStatic");
    println!("cargo:rustc-link-lib=advapi32");
    println!("cargo:rustc-link-lib=ole32");
}
