//! Build and run the libc-less `#[global_allocator]` wasm smokes under wasmtime.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::process::cargo_in_root;
use crate::rust_root;
use crate::wasm::{cargo_tree_has_libc, check_imports, wasm_imports_file, WasmImportPolicy};

fn cargo_target_dir() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| rust_root().join("target"))
}

fn ensure_wasm_targets() -> Result<()> {
    let rustup = which::which("rustup").context("wasm-smoke: rustup not found")?;
    let st = Command::new(rustup)
        .args(["target", "add", "wasm32-unknown-unknown", "wasm32-wasip1"])
        .status()?;
    if !st.success() {
        bail!("rustup target add failed");
    }
    Ok(())
}

fn find_wasmtime() -> Option<PathBuf> {
    if let Ok(p) = which::which("wasmtime") {
        return Some(p);
    }
    if let Ok(nix) = which::which("nix-build") {
        let out = Command::new(nix)
            .args(["--no-out-link", "<nixpkgs>", "-A", "wasmtime"])
            .output()
            .ok()?;
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let bin = PathBuf::from(p).join("bin/wasmtime");
            if bin.is_file() {
                return Some(bin);
            }
        }
    }
    None
}

pub fn run() -> Result<()> {
    ensure_wasm_targets()?;
    let target_dir = cargo_target_dir();

    println!("==> cargo tree (wasm32-unknown-unknown must not include libc)");
    let tree = cargo_in_root()
        .args([
            "tree",
            "-p",
            "mimalloc-wasm-smoke",
            "--target",
            "wasm32-unknown-unknown",
            "--edges",
            "normal",
        ])
        .output()
        .context("cargo tree")?;
    let tree_s = String::from_utf8_lossy(&tree.stdout);
    print!("{tree_s}");
    if cargo_tree_has_libc(&tree_s) {
        bail!("libc must not appear in the wasm32-unknown-unknown crate graph");
    }

    println!("==> build wasm32-unknown-unknown");
    let st = cargo_in_root()
        .args([
            "build",
            "--release",
            "-p",
            "mimalloc-wasm-smoke",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .status()?;
    if !st.success() {
        bail!("wasm unknown-unknown build failed");
    }
    let unknown = target_dir.join("wasm32-unknown-unknown/release/mimalloc-wasm-smoke.wasm");
    if !unknown.is_file() {
        bail!("missing {unknown:?}");
    }

    println!("==> wasm32-unknown-unknown imports (expect none, never libc)");
    let imps = wasm_imports_file(&unknown)?;
    for imp in &imps {
        println!("  import {}.{}", imp.module, imp.name);
    }
    if imps.is_empty() {
        println!("  (no imports)");
    }
    check_imports(&imps, WasmImportPolicy::ExpectNone)
        .map_err(|f| anyhow::anyhow!("forbidden wasm imports: {f:?}"))?;

    println!("==> build wasm32-wasip1");
    let st = cargo_in_root()
        .args([
            "build",
            "--release",
            "-p",
            "mimalloc-wasm-smoke",
            "--target",
            "wasm32-wasip1",
        ])
        .status()?;
    if !st.success() {
        bail!("wasm wasip1 build failed");
    }
    let wasi = target_dir.join("wasm32-wasip1/release/mimalloc-wasm-smoke.wasm");
    if !wasi.is_file() {
        bail!("missing {wasi:?}");
    }

    println!("==> wasm32-wasip1 imports (WASI only, never libc)");
    let imps = wasm_imports_file(&wasi)?;
    for imp in &imps {
        println!("  import {}.{}", imp.module, imp.name);
    }
    check_imports(&imps, WasmImportPolicy::AllowWasi)
        .map_err(|f| anyhow::anyhow!("forbidden wasm imports: {f:?}"))?;

    let Some(wasmtime) = find_wasmtime() else {
        bail!("wasm-smoke: wasmtime not found; built modules but did not execute");
    };

    println!("==> wasmtime wasm32-unknown-unknown --invoke smoke");
    let out = Command::new(&wasmtime)
        .args(["--invoke", "smoke"])
        .arg(&unknown)
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains('0') {
        bail!("smoke() returned: {text}");
    }
    println!("==> wasmtime wasm32-unknown-unknown --invoke stress");
    let out = Command::new(&wasmtime)
        .args(["--invoke", "stress"])
        .arg(&unknown)
        .output()?;
    if !out.status.success() {
        bail!(
            "stress() failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains('0') {
        bail!("stress() returned: {text}");
    }
    let st = Command::new(&wasmtime).arg(&unknown).status()?;
    if !st.success() {
        bail!("wasmtime unknown-unknown failed");
    }

    println!("==> wasmtime wasm32-wasip1");
    let st = Command::new(&wasmtime).arg(&wasi).status()?;
    if !st.success() {
        bail!("wasmtime wasip1 failed");
    }

    println!("==> cargo test mimalloc-core --target wasm32-wasip1");
    if std::env::var_os("CARGO_TARGET_WASM32_WASIP1_RUNNER").is_none() {
        std::env::set_var("CARGO_TARGET_WASM32_WASIP1_RUNNER", &wasmtime);
    }
    let st = cargo_in_root()
        .args(["test", "-p", "mimalloc-core", "--target", "wasm32-wasip1"])
        .status()?;
    if !st.success() {
        bail!("mimalloc-core wasm tests failed");
    }
    let st = cargo_in_root()
        .args([
            "test",
            "-p",
            "mimalloc-wasm-smoke",
            "--target",
            "wasm32-wasip1",
        ])
        .status()?;
    if !st.success() {
        bail!("mimalloc-wasm-smoke tests failed");
    }

    println!("wasm-smoke ok");
    Ok(())
}
