// SPDX-License-Identifier: AGPL-3.0-or-later
//! Link Microsoft's permitted system-API loader into the portable executable.
use std::path::PathBuf;
#[path = "src/build_date.rs"]
mod build_date;
use build_date::build_date;

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../web");
    println!("cargo:rerun-if-changed=../../data/engine/atelier_core_wasm.wasm");
    let seconds = std::env::var("SOURCE_DATE_EPOCH").map_or_else(
        |_| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("build time after epoch")
                .as_secs()
        },
        |value| {
            value
                .parse::<u64>()
                .expect("SOURCE_DATE_EPOCH must be unsigned seconds")
        },
    );
    let date = build_date(seconds);
    let metadata = format!(
        "// SPDX-License-Identifier: AGPL-3.0-or-later\nexport const BUILD_INFO = {{version:\"{}\",buildDate:\"{}\",sourceUrl:\"https://github.com/simkamil/mon-bureau\"}};\n",
        std::env::var("CARGO_PKG_VERSION").expect("Cargo package version"),
        date,
    );
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("build-info.mjs"),
        metadata,
    )
    .expect("write compile-time build metadata");
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
