// Builds crates/injector for i686-pc-windows-msvc and puts the exe in OUT_DIR so lib.rs can
// embed it. OMPTUI_INJECTOR_EXE=<path> embeds a prebuilt exe instead, OMPTUI_SKIP_INJECTOR=1
// embeds nothing.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let dst = out.join("omp-injector.exe");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent().and_then(Path::parent).expect("workspace root").to_path_buf();
    let injector_dir = workspace.join("crates").join("injector");

    println!("cargo:rerun-if-env-changed=OMPTUI_INJECTOR_EXE");
    println!("cargo:rerun-if-env-changed=OMPTUI_SKIP_INJECTOR");
    for f in ["Cargo.toml", ".cargo/config.toml", "src/main.rs", "src/win.rs", "src/rt.rs"] {
        println!("cargo:rerun-if-changed={}", injector_dir.join(f).display());
    }

    if let Some(p) = env::var_os("OMPTUI_INJECTOR_EXE") {
        fs::copy(&p, &dst).unwrap_or_else(|e| panic!("copy {}: {e}", Path::new(&p).display()));
        return;
    }
    if env::var_os("OMPTUI_SKIP_INJECTOR").is_some_and(|v| v != "0") {
        println!("cargo:warning=OMPTUI_SKIP_INJECTOR set: the launcher will have no injector helper embedded");
        fs::write(&dst, b"").expect("write empty helper");
        return;
    }

    let cargo = env::var_os("CARGO").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("cargo"));
    let target_dir = injector_dir.join("target");
    let mut cmd = Command::new(cargo);
    cmd.current_dir(&injector_dir)
        .args(["build", "--release", "--target", "i686-pc-windows-msvc"])
        .arg("--target-dir")
        .arg(&target_dir);
    for (k, _) in env::vars_os() {
        let k = k.to_string_lossy().into_owned();
        if k == "RUSTFLAGS"
            || k == "CARGO_ENCODED_RUSTFLAGS"
            || k == "CARGO_TARGET_DIR"
            || k == "CARGO_BUILD_TARGET"
            || k.starts_with("CARGO_FEATURE_")
            || k.starts_with("CARGO_CFG_")
        {
            cmd.env_remove(&k);
        }
    }
    let status = cmd.status().unwrap_or_else(|e| panic!("could not run cargo for the injector: {e}"));
    if !status.success() {
        panic!(
            "building crates/injector failed. It needs the `i686-pc-windows-msvc` rust target \
             (`rustup target add i686-pc-windows-msvc`) and `lld-link` (package `lld`). \
             Set OMPTUI_SKIP_INJECTOR=1 to build without the helper."
        );
    }
    let built = target_dir.join("i686-pc-windows-msvc").join("release").join("omp-injector.exe");
    fs::copy(&built, &dst).unwrap_or_else(|e| panic!("copy {}: {e}", built.display()));
}
