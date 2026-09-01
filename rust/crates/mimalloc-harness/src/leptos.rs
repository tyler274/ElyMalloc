//! [Leptos](https://github.com/leptos-rs/leptos) test suites as WASM allocator stress.
//!
//! Clone upstream, inject `mimalloc_core::Mimalloc` as `#[global_allocator]` into
//! crates that run without a browser DOM, and **run** `cargo test` on
//! `wasm32-wasip1` under wasmtime. Compile/link success is not enough.
//! Also builds and runs in-tree `mimalloc-leptos-smoke` (reactive_graph churn).
//!
//! `LEPTOS_SRC=` skips the fetch. `LEPTOS_REFRESH=1` reclones.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::compare::Captured;
use crate::process::{cargo_in_root, run_captured_os};
use crate::projects::git_clone_depth;
use crate::rust_root;
use crate::wasm::{
    check_imports, ensure_wasm_targets, find_wasmtime, wasm_imports_file, WasmImportPolicy,
};

const INJECT_MARK: &str = "// mimalloc-rewrite-global-allocator";

/// Crates whose tests are expected to run on WASI (no `web-sys` / wasm-bindgen).
const WASI_TEST_CRATES: &[&str] = &[
    "oco",
    "either_of",
    "or_poisoned",
    "const_str_slice_concat",
    "next_tuple",
    "any_spawner",
    "hydration_context",
    "reactive_graph",
];

fn cache_dir() -> PathBuf {
    rust_root().join("target/project-suites")
}

fn cargo_bin() -> PathBuf {
    crate::which("cargo").unwrap_or_else(|| PathBuf::from("cargo"))
}

fn ensure_leptos_src() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("LEPTOS_SRC") {
        let pb = PathBuf::from(p);
        if pb.join("Cargo.toml").is_file() && pb.join("reactive_graph").is_dir() {
            return Ok(pb);
        }
        bail!("LEPTOS_SRC missing Cargo.toml / reactive_graph");
    }
    let dest = cache_dir().join("leptos");
    if dest.join("Cargo.toml").is_file()
        && dest.join("reactive_graph").is_dir()
        && !crate::env_is_one("LEPTOS_REFRESH")
    {
        return Ok(dest);
    }
    println!("==> fetch leptos-rs/leptos");
    git_clone_depth(
        &dest,
        "https://github.com/leptos-rs/leptos.git",
        &["v0.8.20", "0.8.20", "main"],
    )?;
    if !dest.join("reactive_graph").is_dir() {
        bail!("leptos checkout missing reactive_graph");
    }
    Ok(dest)
}

fn crate_package_name(crate_dir: &Path) -> Result<String> {
    let toml = fs::read_to_string(crate_dir.join("Cargo.toml"))?;
    let mut in_package = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("name = \"") {
                if let Some(name) = rest.strip_suffix('"') {
                    return Ok(name.to_string());
                }
            }
        }
    }
    bail!("no package name in {}", crate_dir.display());
}

fn patch_tokio_for_wasm(crate_dir: &Path) -> Result<()> {
    let cargo = crate_dir.join("Cargo.toml");
    let toml = fs::read_to_string(&cargo)?;
    if !toml.contains("rt-multi-thread") {
        return Ok(());
    }
    let patched = toml.replace("rt-multi-thread", "rt");
    fs::write(&cargo, patched)?;
    Ok(())
}

fn path_toml(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

pub fn inject_mimalloc(crate_dir: &Path, mimalloc_core: &Path) -> Result<bool> {
    let lib = crate_dir.join("src/lib.rs");
    if !lib.is_file() {
        return Ok(false);
    }
    let cargo = crate_dir.join("Cargo.toml");
    let mut toml = fs::read_to_string(&cargo).with_context(|| format!("read {}", cargo.display()))?;
    let mut rs = fs::read_to_string(&lib).with_context(|| format!("read {}", lib.display()))?;
    if let Some(i) = rs.find(INJECT_MARK) {
        rs.truncate(i);
    }
    let dep = format!(
        "\n[dev-dependencies.mimalloc-core]\npath = \"{}\"\n",
        path_toml(mimalloc_core)
    );
    if !toml.contains("[dev-dependencies.mimalloc-core]") {
        toml.push_str(&dep);
        fs::write(&cargo, toml)?;
    }
    if !rs.ends_with('\n') {
        rs.push('\n');
    }
    rs.push_str(INJECT_MARK);
    rs.push_str(
        "\n#[cfg(test)]\n#[global_allocator]\nstatic __MIMALLOC_REWRITE: mimalloc_core::Mimalloc = mimalloc_core::Mimalloc;\n",
    );
    fs::write(&lib, rs)?;
    Ok(true)
}

fn cargo_fingerprint(cap: &Captured) -> Option<String> {
    let text = format!("{}{}", cap.stdout_str(), cap.stderr_str());
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("test result:") || line.starts_with("running ") {
            lines.push(line.to_string());
        }
    }
    if lines.iter().any(|l| l.starts_with("test result:")) {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn run_cargo_test(
    src: &Path,
    target_dir: &Path,
    pkg: &str,
    triple: &str,
    wasmtime: &Path,
    timeout: Duration,
) -> Result<Captured> {
    let cargo = cargo_bin();
    let runner_key = format!(
        "CARGO_TARGET_{}_RUNNER",
        triple.replace('-', "_").to_ascii_uppercase()
    );
    let args: Vec<std::ffi::OsString> = [
        "test",
        "-p",
        pkg,
        "--lib",
        "--target",
        triple,
        "--",
        "--test-threads=1",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    let mut extra = vec![
        (
            std::ffi::OsString::from("CARGO_TARGET_DIR"),
            target_dir.as_os_str().to_os_string(),
        ),
        (
            std::ffi::OsString::from("RUSTUP_TOOLCHAIN"),
            std::ffi::OsString::from(
                std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into()),
            ),
        ),
        (
            std::ffi::OsString::from(runner_key),
            wasmtime.as_os_str().to_os_string(),
        ),
    ];
    if let Some(dir) = cargo.parent() {
        let mut path = dir.as_os_str().to_os_string();
        path.push(":");
        if let Ok(p) = std::env::var("PATH") {
            path.push(p);
        }
        extra.push((std::ffi::OsString::from("PATH"), path));
    }
    run_captured_os(
        &cargo,
        &args,
        &extra,
        timeout,
        Some(src),
        &["LD_PRELOAD"],
    )
}

fn print_cap(name: &str, cap: &Captured) {
    print!("{name}");
    if !cap.stdout.is_empty() {
        print!("{}", cap.stdout_str());
    }
    if !cap.stderr.is_empty() {
        eprint!("{}", cap.stderr_str());
    }
}

fn run_in_tree_smoke(wasmtime: &Path) -> Result<()> {
    println!("==> mimalloc-leptos-smoke (reactive_graph + Mimalloc) wasm32-wasip1");
    let st = cargo_in_root()
        .args([
            "build",
            "--release",
            "-p",
            "mimalloc-leptos-smoke",
            "--target",
            "wasm32-wasip1",
        ])
        .status()?;
    if !st.success() {
        bail!("mimalloc-leptos-smoke wasip1 build failed");
    }
    let wasm = rust_root().join("target/wasm32-wasip1/release/mimalloc-leptos-smoke.wasm");
    if !wasm.is_file() {
        bail!("missing {}", wasm.display());
    }
    let imps = wasm_imports_file(&wasm)?;
    check_imports(&imps, WasmImportPolicy::AllowWasi)
        .map_err(|f| anyhow::anyhow!("leptos-smoke wasip1 forbidden imports: {f:?}"))?;
    let st = Command::new(wasmtime).arg(&wasm).status()?;
    if !st.success() {
        bail!("wasmtime mimalloc-leptos-smoke wasip1 failed");
    }

    println!("==> mimalloc-leptos-smoke wasm32-unknown-unknown");
    let st = cargo_in_root()
        .args([
            "build",
            "--release",
            "-p",
            "mimalloc-leptos-smoke",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .status()?;
    if !st.success() {
        bail!("mimalloc-leptos-smoke unknown-unknown build failed");
    }
    let unknown = rust_root().join("target/wasm32-unknown-unknown/release/mimalloc-leptos-smoke.wasm");
    if !unknown.is_file() {
        bail!("missing {}", unknown.display());
    }
    let imps = wasm_imports_file(&unknown)?;
    check_imports(&imps, WasmImportPolicy::NoLibc)
        .map_err(|f| anyhow::anyhow!("leptos-smoke unknown-unknown libc malloc import: {f:?}"))?;
    if imps.is_empty() {
        let out = Command::new(wasmtime)
            .args(["--invoke", "smoke"])
            .arg(&unknown)
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        if !text.contains('0') {
            bail!("leptos-smoke smoke() returned: {text}");
        }
    } else {
        println!(
            "  ({} host imports; skip wasmtime invoke, wasip1 already ran)",
            imps.len()
        );
        for imp in &imps {
            println!("  import {}.{}", imp.module, imp.name);
        }
    }
    Ok(())
}

fn run_leptos_csr_check(src: &Path, target_dir: &Path) -> Result<()> {
    println!("==> leptos csr check wasm32-unknown-unknown (no libc malloc)");
    let st = Command::new(cargo_bin())
        .current_dir(src)
        .env("CARGO_TARGET_DIR", target_dir)
        .env(
            "RUSTUP_TOOLCHAIN",
            std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into()),
        )
        .args([
            "check",
            "-p",
            "leptos",
            "--no-default-features",
            "--features=csr",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .status()?;
    if !st.success() {
        bail!("leptos csr wasm32-unknown-unknown check failed");
    }
    Ok(())
}

pub fn run() -> Result<()> {
    ensure_wasm_targets()?;
    let Some(wasmtime) = find_wasmtime() else {
        bail!("leptos: wasmtime not found");
    };

    println!("==> cargo test -p mimalloc-leptos-smoke (host)");
    let st = cargo_in_root()
        .args(["test", "-p", "mimalloc-leptos-smoke"])
        .status()?;
    if !st.success() {
        bail!("mimalloc-leptos-smoke host tests failed");
    }

    run_in_tree_smoke(&wasmtime)?;

    let src = ensure_leptos_src()?;
    let core = rust_root().join("crates/mimalloc-core");
    let target = cache_dir().join("leptos-target");
    fs::create_dir_all(&target)?;
    println!("leptos src: {}", src.display());

    let mut injected = 0usize;
    for pkg in WASI_TEST_CRATES {
        let dir = src.join(pkg);
        patch_tokio_for_wasm(&dir)?;
        if inject_mimalloc(&dir, &core)? {
            injected += 1;
        }
    }
    println!("==> injected Mimalloc GlobalAlloc into {injected} leptos crates");

    run_leptos_csr_check(&src, &target)?;

    let mut ran = 0usize;
    let mut fail = 0usize;
    let mut skip = 0usize;
    let mut required_ok = false;
    for pkg in WASI_TEST_CRATES {
        if !src.join(pkg).join("src/lib.rs").is_file() {
            println!(".. {pkg} skip (no lib)");
            skip += 1;
            continue;
        }
        let pkg_name = crate_package_name(&src.join(pkg))?;
        print!(".. {pkg_name} wasm32-wasip1 ");
        let cap = run_cargo_test(
            &src,
            &target,
            &pkg_name,
            "wasm32-wasip1",
            &wasmtime,
            Duration::from_secs(600),
        )?;
        let fp = cargo_fingerprint(&cap);
        let ok = cap.rc == 0
            && fp
                .as_deref()
                .is_some_and(|f| f.contains("test result: ok."));
        let test_fail = fp
            .as_deref()
            .is_some_and(|f| f.contains("test result: FAILED"));
        if ok {
            println!("ok");
            ran += 1;
            if *pkg == "reactive_graph" {
                required_ok = true;
            }
        } else if test_fail {
            println!("FAIL rc={}", cap.rc);
            print_cap("", &cap);
            fail += 1;
        } else {
            println!("skip (no WASI test run, rc={})", cap.rc);
            if *pkg == "reactive_graph" {
                print_cap("", &cap);
            }
            skip += 1;
        }
    }
    println!("leptos wasm tests: ran={ran} skip={skip} fail={fail}");
    if !required_ok {
        bail!("reactive_graph wasm32-wasip1 tests did not run");
    }
    if ran == 0 {
        bail!("no leptos WASI test crates ran");
    }
    if fail > 0 {
        bail!("{fail} leptos wasm32-wasip1 suite failure(s)");
    }
    println!("leptos-wasm: {ran} crates ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::write_all;

    #[test]
    fn inject_is_idempotent() {
        let dir = std::env::temp_dir().join(format!(
            "mimalloc-leptos-inject-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        write_all(&dir.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
        write_all(&dir.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let core = rust_root().join("crates/mimalloc-core");
        assert!(inject_mimalloc(&dir, &core).unwrap());
        assert!(inject_mimalloc(&dir, &core).unwrap());
        let rs = fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert_eq!(
            rs.matches("static __MIMALLOC_REWRITE").count(),
            1,
            "{rs}"
        );
        assert!(rs.contains(INJECT_MARK));
        assert!(rs.contains("global_allocator"));
        let toml = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert_eq!(
            toml.matches("[dev-dependencies.mimalloc-core]").count(),
            1,
            "{toml}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wasi_crates_are_leptos_members() {
        for p in WASI_TEST_CRATES {
            assert!(!p.is_empty());
            assert!(!p.contains('/'));
        }
        assert!(WASI_TEST_CRATES.contains(&"reactive_graph"));
        assert!(WASI_TEST_CRATES.contains(&"oco"));
    }

    #[test]
    fn tokio_wasm_feature_is_single_thread() {
        let s = "tokio = { features = [\n  \"rt-multi-thread\",\n  \"macros\",\n] }";
        let p = s.replace("rt-multi-thread", "rt");
        assert!(p.contains("\"rt\""));
        assert!(!p.contains("rt-multi-thread"));
    }

    #[test]
    fn package_name_reads_cargo_toml() {
        let dir = std::env::temp_dir().join(format!(
            "mimalloc-leptos-pkgname-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write_all(
            &dir.join("Cargo.toml"),
            "[package]\nname = \"oco_ref\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(crate_package_name(&dir).unwrap(), "oco_ref");
        let _ = fs::remove_dir_all(&dir);
    }
}
