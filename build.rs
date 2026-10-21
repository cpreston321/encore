//! Compiles native/EncoreNative.swift into libencore_native.dylib next to the binary.
//! Encore loads it at runtime and falls back to the web player when it's missing, so a
//! failed Swift build is a warning, not an error.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=native/EncoreNative.swift");
    // The release workflow builds a developer token in (see `api::built_in_token`).
    println!("cargo:rerun-if-env-changed=ENCORE_DEVELOPER_TOKEN");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // OUT_DIR is target/<profile>/build/<pkg>-<hash>/out.
    let Some(profile_dir) = out_dir.ancestors().nth(3) else { return };
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok(other) => other,
        Err(_) => return,
    }
    .to_string();
    let dylib = profile_dir.join("libencore_native.dylib");
    let status = Command::new("xcrun")
        .args(["swiftc", "-O", "-swift-version", "5", "-emit-library", "-module-name", "EncoreNative"])
        .arg("-target")
        .arg(format!("{arch}-apple-macos13.0"))
        .arg("native/EncoreNative.swift")
        .arg("-o")
        .arg(&dylib)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => println!("cargo:warning=building the native player failed ({s}); full songs will use the web player"),
        Err(e) => println!("cargo:warning=swiftc not found ({e}); full songs will use the web player"),
    }
}
