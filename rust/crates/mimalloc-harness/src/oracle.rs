use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::compare::outputs_match;
use crate::failset::{fail_names, only_in_left, rustc_fail_names};
use crate::process::{compile, run_captured, run_captured_preload};
use crate::{env_is_one, repo_root, rust_root};

pub const JEMALLOC_CANDIDATES: &[&str] = &[
    "/usr/lib64/libjemalloc.so",
    "/usr/lib/libjemalloc.so",
    "/usr/lib/x86_64-linux-gnu/libjemalloc.so.2",
    "/usr/lib64/libjemalloc.so.2",
];

pub fn find_jemalloc_among(
    env_so: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(p) = env_so {
        if exists(p) {
            return Some(p.to_path_buf());
        }
    }
    for c in JEMALLOC_CANDIDATES {
        let p = Path::new(c);
        if exists(p) {
            return Some(p.to_path_buf());
        }
    }
    None
}

fn find_jemalloc() -> Result<PathBuf> {
    if let Some(p) = find_jemalloc_among(
        std::env::var_os("JEMALLOC_SO")
            .map(PathBuf::from)
            .as_deref(),
        |p| p.is_file(),
    ) {
        return Ok(p);
    }
    if let Ok(nix) = which::which("nix-build") {
        let out = Command::new(nix)
            .args(["--no-out-link", "<nixpkgs>", "-A", "jemalloc"])
            .output()?;
        if out.status.success() {
            let store = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let so = PathBuf::from(&store).join("lib/libjemalloc.so");
            if so.is_file() {
                return Ok(so);
            }
        }
    }
    bail!("oracle-suites: stock jemalloc not found (set JEMALLOC_SO)");
}

pub fn try_jemalloc() -> Option<PathBuf> {
    find_jemalloc().ok()
}

/// C mimalloc built with `MI_SECURE=FULL` (cached under `target/compiler-stress`).
pub fn c_mimalloc_secure_so() -> Result<PathBuf> {
    let cache = rust_root().join("target/compiler-stress");
    std::fs::create_dir_all(&cache)?;
    build_c_oracle(&cache, &repo_root())
}

fn build_c_oracle(cache: &Path, repo: &Path) -> Result<PathBuf> {
    let dest = cache.join("c-oracle");
    let cmake =
        which::which("cmake").context("oracle-suites: cmake is required to build C mimalloc")?;
    println!("==> build C mimalloc oracle (MI_SECURE=FULL)");
    let st = Command::new(&cmake)
        .args([
            "-S",
            repo.to_str().unwrap(),
            "-B",
            dest.to_str().unwrap(),
            "-DMI_SECURE=FULL",
            "-DMI_BUILD_TESTS=OFF",
            "-DMI_BUILD_OBJECT=OFF",
            "-DMI_BUILD_STATIC=OFF",
            "-DMI_OVERRIDE=ON",
            "-DMI_OVERRIDE_LIBC_EXTRAS=OFF",
            "-DCMAKE_BUILD_TYPE=Release",
        ])
        .status()?;
    if !st.success() {
        bail!("cmake configure failed");
    }
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "4".into());
    let st = Command::new(&cmake)
        .args(["--build", dest.to_str().unwrap(), "-j", &jobs])
        .status()?;
    if !st.success() {
        bail!("cmake build failed");
    }
    for name in ["libmimalloc-secure.so", "libmimalloc.so"] {
        if let Ok(p) = glob_so(&dest, name) {
            return Ok(p);
        }
    }
    bail!("C mimalloc shared library not found in {}", dest.display());
}

fn glob_so(dir: &Path, name: &str) -> Result<PathBuf> {
    for e in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_name() == name {
            return Ok(e.into_path());
        }
    }
    bail!("no {name}");
}

pub fn run() -> Result<()> {
    let rust = rust_root();
    let repo = repo_root();
    let cache = rust.join("target/compiler-stress");
    std::fs::create_dir_all(&cache)?;
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        std::env::set_var("CARGO_TARGET_DIR", rust.join("target"));
    }
    let suites = std::env::var("SUITES").unwrap_or_else(|_| "all".into());

    println!("==> build Rust libmimalloc.so and libmimalloc-secure.so");
    let (rust_so, rust_secure_so) = crate::process::build_mimalloc_cdylibs()?;
    let c_so = build_c_oracle(&cache, &repo)?;
    let je_so = find_jemalloc()?;
    println!("rust so:        {}", rust_so.display());
    println!("rust secure so: {}", rust_secure_so.display());
    println!("c so:           {}", c_so.display());
    println!("jemalloc so:    {}", je_so.display());

    if suites != "rustc" {
        run_c_abi("rust", &rust_so, false, false)?;
        run_c_abi("rust-secure", &rust_secure_so, false, false)?;
        run_c_abi("c", &c_so, true, true)?;
    }

    println!("==> host smoke / stress: same binaries, Rust vs C vs jemalloc");
    let smoke = cache.join("mi-smoke");
    compile(
        "cc",
        &[
            "-O2",
            "-pthread",
            rust.join("tests/smoke.c").to_str().unwrap(),
        ],
        &smoke,
    )?;
    let inc = format!("-I{}", repo.join("include").display());
    let stress_src = repo.join("test/test-stress.c");
    let stress = cache.join("mi-stress");
    compile(
        "cc",
        &[
            "-O2",
            "-pthread",
            "-DUSE_STD_MALLOC",
            "-DNDEBUG",
            &inc,
            stress_src.to_str().unwrap(),
        ],
        &stress,
    )?;

    oracle_run_match("rust-smoke", &rust_so, &smoke, &cache)?;
    oracle_run_match("rust-secure-smoke", &rust_secure_so, &smoke, &cache)?;
    oracle_run_match("c-smoke", &c_so, &smoke, &cache)?;
    oracle_run_match("jemalloc-smoke", &je_so, &smoke, &cache)?;
    for (name, so) in [
        ("rust-stress", &rust_so),
        ("rust-secure-stress", &rust_secure_so),
        ("c-stress", &c_so),
        ("jemalloc-stress", &je_so),
    ] {
        let cap = run_captured_preload(so, &stress, &["2", "4", "2"], Duration::from_secs(60))?;
        if cap.rc != 0 {
            bail!("FAIL {name}");
        }
        println!("  ok   {name}");
    }

    crate::linkers::run_global_alloc_per_linker(&cache)?;
    crate::linkers::stress_linkers_under_preload(&rust_so, &rust_secure_so, &c_so, &je_so, &cache)?;

    let rustc_only = suites == "rustc";
    let je_skip_c = !env_is_one("JEMALLOC_FULL");
    let rust_rc = run_preload("rust", &rust_so, rustc_only, false, false)?;
    let c_rc = run_preload("c", &c_so, rustc_only, false, true)?;
    let je_rc = run_preload("jemalloc", &je_so, je_skip_c, true, true)?;

    let rust_fail = cache.join("results/rust/fail.txt");
    let c_fail = cache.join("results/c/fail.txt");
    let _je_fail = cache.join("results/jemalloc/fail.txt");
    let rust_txt = std::fs::read_to_string(&rust_fail).unwrap_or_default();
    let c_txt = std::fs::read_to_string(&c_fail).unwrap_or_default();

    println!("==> oracle FAIL-set diff (Rust vs C, GCC/Clang/host)");
    let rust_names = fail_names(&rust_txt);
    let c_names = fail_names(&c_txt);
    let extra = only_in_left(&rust_names, &c_names);
    let mut diff_rc = 0i32;
    if rust_names == c_names {
        println!("  FAIL sets identical ({} names)", rust_names.len());
    } else {
        println!("  only in Rust:");
        for n in &extra {
            println!("  {n}");
        }
        println!("  only in C:");
        for n in only_in_left(&c_names, &rust_names) {
            println!("  {n}");
        }
        if extra.is_empty() {
            println!("  no Rust-only failures (C-only failures are tolerated)");
        } else {
            eprintln!("Rust allocator failed tests that C mimalloc passed");
            diff_rc = 1;
        }
    }

    println!("==> rustc three-way runtime (same binaries; Rust vs C mimalloc vs stock jemalloc)");
    let mut rustc_rc = 0i32;
    if !rustc_vs(
        "C mimalloc",
        &cache.join("results/c"),
        &cache.join("results/rust"),
    ) {
        rustc_rc = 1;
    }
    if !rustc_vs(
        "jemalloc",
        &cache.join("results/jemalloc"),
        &cache.join("results/rust"),
    ) {
        rustc_rc = 1;
    }

    if rust_rc != 0 && c_rc == 0 {
        bail!("Rust compiler-preload failed while C oracle passed");
    }
    if diff_rc != 0 || rustc_rc != 0 {
        bail!("oracle FAIL-set or rustc runtime mismatch");
    }

    println!();
    println!(
        "oracle-suites ok (c-abi/cxx match; compiler FAIL sets have no Rust-only regressions;"
    );
    println!("  rustc UI binaries were compiled once and must match system-malloc output under each .so)");
    println!("  rust     compiler-preload exit={rust_rc}");
    println!("  c        compiler-preload exit={c_rc}");
    println!("  jemalloc compiler-preload exit={je_rc}");
    Ok(())
}

fn run_c_abi(tag: &str, so: &Path, skip_rust: bool, skip_cxx: bool) -> Result<()> {
    println!("==> C ABI + C++ ({tag})");
    let rust = rust_root();
    let out = rust
        .join("target/compiler-stress")
        .join(format!("c-abi-{tag}"));
    std::fs::create_dir_all(&out)?;
    std::env::set_var("SO", so);
    std::env::set_var("INCLUDE", repo_root().join("include"));
    std::env::set_var("C_TESTS", rust.join("tests"));
    std::env::set_var("UPSTREAM_TESTS", repo_root().join("test"));
    std::env::set_var("DEBUG_SO", "");
    std::env::set_var("OUT", &out);
    std::env::set_var("SKIP_RUST_ONLY", if skip_rust { "1" } else { "0" });
    std::env::set_var("SKIP_CXX", if skip_cxx { "1" } else { "0" });
    crate::cabi::run()?;
    println!("  ok   c-abi {tag}");
    Ok(())
}

fn run_preload(tag: &str, so: &Path, skip_c: bool, skip_mi: bool, reuse: bool) -> Result<i32> {
    println!("==> compiler suites under {tag} (run + match system-malloc output)");
    std::env::set_var("SKIP_BUILD", "1");
    std::env::set_var("SKIP_C_TORTURE", if skip_c { "1" } else { "0" });
    std::env::set_var("SKIP_MI_API", if skip_mi { "1" } else { "0" });
    std::env::set_var("REUSE_BINS", if reuse { "1" } else { "0" });
    std::env::set_var("SO", so);
    std::env::set_var(
        "RESULT_DIR",
        rust_root().join("target/compiler-stress/results").join(tag),
    );
    match crate::preload::run() {
        Ok(()) => Ok(0),
        Err(e) => {
            eprintln!("{e:#}");
            Ok(1)
        }
    }
}

fn oracle_run_match(name: &str, so: &Path, bin: &Path, cache: &Path) -> Result<()> {
    let dir = cache.join("host-out");
    std::fs::create_dir_all(&dir)?;
    let stem = format!("{}.sys", bin.file_name().unwrap().to_string_lossy());
    let sys_base = dir.join(&stem);
    let rc_path = PathBuf::from(format!("{}.rc", sys_base.display()));
    let sys = if rc_path.is_file() {
        crate::compare::Captured {
            stdout: std::fs::read(format!("{}.stdout", sys_base.display())).unwrap_or_default(),
            stderr: std::fs::read(format!("{}.stderr", sys_base.display())).unwrap_or_default(),
            rc: std::fs::read_to_string(format!("{}.rc", sys_base.display()))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(255),
        }
    } else {
        let cap = run_captured(bin, &[], &[], Duration::from_secs(60))?;
        std::fs::write(format!("{}.stdout", sys_base.display()), &cap.stdout)?;
        std::fs::write(format!("{}.stderr", sys_base.display()), &cap.stderr)?;
        std::fs::write(
            format!("{}.rc", sys_base.display()),
            format!("{}\n", cap.rc),
        )?;
        cap
    };
    let got = run_captured_preload(so, bin, &[], Duration::from_secs(60))?;
    if outputs_match(&sys, &got) {
        println!("  ok   {name} (output matches system malloc)");
        return Ok(());
    }
    bail!("FAIL {name} (output/rc != system malloc)");
}

fn rustc_vs(label: &str, base: &Path, rust_dir: &Path) -> bool {
    let rust_txt = std::fs::read_to_string(rust_dir.join("fail.txt")).unwrap_or_default();
    let base_txt = std::fs::read_to_string(base.join("fail.txt")).unwrap_or_default();
    let rf = rustc_fail_names(&rust_txt);
    let bf = rustc_fail_names(&base_txt);
    println!("==> rustc runtime vs {label} (PASS = matching stdout/stderr/rc vs system malloc)");
    println!("  rustc FAIL rust={} {label}={}", rf.len(), bf.len());
    let extra = only_in_left(&rf, &bf);
    if extra.is_empty() {
        println!("  no Rust-only rustc runtime mismatches vs {label}");
        true
    } else {
        println!("  rustc output mismatches under Rust but not {label}:");
        for n in extra {
            println!("    {n}");
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jemalloc_prefers_env() {
        let env = Path::new("/tmp/je.so");
        let found = find_jemalloc_among(Some(env), |p| p == env);
        assert_eq!(found.as_deref(), Some(env));
    }

    #[test]
    fn jemalloc_walks_candidates() {
        let found = find_jemalloc_among(None, |p| p == Path::new("/usr/lib/libjemalloc.so"));
        assert_eq!(found.as_deref(), Some(Path::new("/usr/lib/libjemalloc.so")));
    }
}
