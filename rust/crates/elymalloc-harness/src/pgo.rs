//! LLVM PGO: train instrumented ElyMalloc, or run generate/train/merge/use.
//!
//! `pgo-train` runs the same workloads as the old shell driver: `elymalloc-bench`,
//! `elymalloc-alloc-stress`, `tests/{smoke,bench}.c` under `LD_PRELOAD`, and
//! `chaos.c` **linked** against the cdylib (`mi_*` are not libc intercepts).
//! The caller must set `LLVM_PROFILE_FILE`. `pgo` locates `llvm-profdata`,
//! builds instrumented crates, trains, merges, and rebuilds with `-Cprofile-use`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::process::{cargo_ok_env, compile, run_ok};
use crate::{repo_root, rust_root, which};

/// Flags for `elymalloc-harness pgo-train`.
#[derive(Args, Debug, Clone, Default)]
pub struct TrainArgs {
    /// Instrumented `libmimalloc.so` (C ABI / `LD_PRELOAD`)
    #[arg(long)]
    pub so: Option<PathBuf>,
    /// Instrumented secure SONAME (`--features secure`)
    #[arg(long)]
    pub secure_so: Option<PathBuf>,
    /// Instrumented `elymalloc-bench` binary
    #[arg(long)]
    pub bench: Option<PathBuf>,
    /// Instrumented `elymalloc-alloc-stress` binary
    #[arg(long)]
    pub stress: Option<PathBuf>,
    /// C compiler for smoke/bench/chaos
    #[arg(long)]
    pub cc: Option<PathBuf>,
    /// Path to `include/` (`mimalloc.h`) for `chaos.c`
    #[arg(long)]
    pub include: Option<PathBuf>,
    /// Path to `rust/tests` (`smoke.c`, `bench.c`, `chaos.c`)
    #[arg(long)]
    pub c_tests: Option<PathBuf>,
    /// Scratch dir for compiled C probes
    #[arg(long)]
    pub tmp: Option<PathBuf>,
}

fn opt_path(flag: Option<PathBuf>, env: &str) -> Option<PathBuf> {
    flag.or_else(|| std::env::var_os(env).map(PathBuf::from))
}

fn join_rustflags(base: &str, extra: &str) -> String {
    if base.is_empty() {
        extra.to_string()
    } else {
        format!("{base} {extra}")
    }
}

fn host_from_verbose(v: &str) -> Option<String> {
    for line in v.lines() {
        if let Some(h) = line.strip_prefix("host: ") {
            return Some(h.trim().to_string());
        }
    }
    None
}

fn rustc_host() -> Result<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .context("rustc -vV")?;
    if !out.status.success() {
        bail!("rustc -vV failed");
    }
    host_from_verbose(&String::from_utf8_lossy(&out.stdout)).context("no host: in rustc -vV")
}

fn rustc_sysroot() -> Result<PathBuf> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .context("rustc --print sysroot")?;
    if !out.status.success() {
        bail!("rustc --print sysroot failed");
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

fn find_llvm_profdata() -> Result<PathBuf> {
    if let Some(p) = which("llvm-profdata") {
        return Ok(p);
    }
    let host = rustc_host()?;
    let p = rustc_sysroot()?
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join("llvm-profdata");
    if p.is_file() {
        return Ok(p);
    }
    bail!("llvm-profdata not found (rustup component add llvm-tools-preview)")
}

fn cdylib_name() -> &'static str {
    #[cfg(windows)]
    {
        "mimalloc.dll"
    }
    #[cfg(target_os = "macos")]
    {
        "libmimalloc.dylib"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "libmimalloc.so"
    }
}

fn bin_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn release_file(target_dir: &Path, name: &str) -> PathBuf {
    let direct = target_dir.join("release").join(name);
    if direct.is_file() {
        return direct;
    }
    if let Ok(host) = rustc_host() {
        let triple = target_dir.join(host).join("release").join(name);
        if triple.is_file() {
            return triple;
        }
    }
    direct
}

fn stack_preload(so: &Path, existing: Option<&OsStr>) -> OsString {
    let mut v = so.as_os_str().to_os_string();
    if let Some(prev) = existing {
        #[cfg(windows)]
        {
            v.push(";");
        }
        #[cfg(not(windows))]
        {
            v.push(":");
        }
        v.push(prev);
    }
    v
}

fn preload_env(so: &Path) -> (&'static str, OsString) {
    #[cfg(target_os = "macos")]
    {
        (
            "DYLD_INSERT_LIBRARIES",
            stack_preload(so, std::env::var_os("DYLD_INSERT_LIBRARIES").as_deref()),
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        (
            "LD_PRELOAD",
            stack_preload(so, std::env::var_os("LD_PRELOAD").as_deref()),
        )
    }
    #[cfg(windows)]
    {
        (
            "PATH",
            stack_preload(
                so.parent().unwrap_or(so),
                std::env::var_os("PATH").as_deref(),
            ),
        )
    }
}

fn lib_dir_env(so: &Path) -> (&'static str, OsString) {
    let dir = so.parent().unwrap_or(so);
    #[cfg(target_os = "macos")]
    {
        (
            "DYLD_LIBRARY_PATH",
            stack_preload(dir, std::env::var_os("DYLD_LIBRARY_PATH").as_deref()),
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        (
            "LD_LIBRARY_PATH",
            stack_preload(dir, std::env::var_os("LD_LIBRARY_PATH").as_deref()),
        )
    }
    #[cfg(windows)]
    {
        (
            "PATH",
            stack_preload(dir, std::env::var_os("PATH").as_deref()),
        )
    }
}

fn run_so(so: &Path, bin: &Path, extra: &[(&str, OsString)]) -> Result<()> {
    if !so.is_file() {
        eprintln!("pgo-train: skip missing {}", so.display());
        return Ok(());
    }
    let preload = preload_env(so);
    let lpath = lib_dir_env(so);
    let mut env: Vec<(&str, OsString)> = Vec::with_capacity(extra.len() + 2);
    env.push(preload);
    env.push(lpath);
    env.extend(extra.iter().cloned());
    run_ok(bin, &[], &env)
}

/// `chaos.c` includes `mimalloc.h` and calls `mi_*` (unlike smoke/bench, which
/// use libc `malloc` under `LD_PRELOAD`). Link the cdylib the same way `c-abi` does.
fn compile_chaos(cc: &Path, include: &Path, src: &Path, so: &Path, out: &Path) -> Result<()> {
    let inc = format!("-I{}", include.display());
    let rpath = format!(
        "-Wl,-rpath,{}",
        so.parent().unwrap_or(so).display()
    );
    compile(
        cc,
        &[
            "-O2",
            "-pthread",
            &inc,
            &rpath,
            utf8(src)?,
            utf8(so)?,
        ],
        out,
    )
}

fn chaos_steps(default: &str) -> OsString {
    std::env::var_os("MIMALLOC_CHAOS_STEPS").unwrap_or_else(|| OsString::from(default))
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("non-utf8 path {}", path.display()))
}

fn collect_profraw(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    let rd = std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))?;
    for e in rd {
        let e = e?;
        if e.path().extension().is_some_and(|x| x == "profraw") {
            v.push(e.path());
        }
    }
    v.sort();
    Ok(v)
}

fn cargo_pgo(args: &[&str], rustflags: &str) -> Result<()> {
    cargo_ok_env(
        args,
        &[
            ("CARGO_INCREMENTAL", OsString::from("0")),
            ("RUSTFLAGS", OsString::from(rustflags)),
        ],
        &[],
    )
}

/// Train instrumented artifacts. Caller must set `LLVM_PROFILE_FILE`.
pub fn train(args: TrainArgs) -> Result<()> {
    if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
        eprintln!("pgo-train: LLVM_PROFILE_FILE is unset; profiles may be incomplete");
    }

    let bench = opt_path(args.bench, "BENCH");
    let stress = opt_path(args.stress, "STRESS");
    let so = opt_path(args.so, "SO");
    let secure_so = opt_path(args.secure_so, "SECURE_SO");
    let cc = opt_path(args.cc, "CC").unwrap_or_else(|| PathBuf::from("cc"));
    let include = opt_path(args.include, "INCLUDE").unwrap_or_else(|| repo_root().join("include"));
    let c_tests = opt_path(args.c_tests, "C_TESTS").unwrap_or_else(|| rust_root().join("tests"));
    let tmp =
        opt_path(args.tmp, "PGO_TRAIN_TMP").unwrap_or_else(|| rust_root().join("target/pgo-train"));

    if let Some(bench) = bench {
        if bench.is_file() {
            eprintln!("pgo-train: elymalloc-bench");
            run_ok(&bench, &[], &[])?;
        }
    }
    if let Some(stress) = stress {
        if stress.is_file() {
            eprintln!("pgo-train: elymalloc-alloc-stress");
            run_ok(&stress, &[], &[])?;
        }
    }

    let Some(so) = so.filter(|p| p.is_file()) else {
        eprintln!("pgo-train: SO not set; rust-only training");
        return Ok(());
    };

    let smoke_src = c_tests.join("smoke.c");
    let bench_src = c_tests.join("bench.c");
    if !smoke_src.is_file() || !bench_src.is_file() {
        eprintln!(
            "pgo-train: skip C tests (missing {} or {})",
            smoke_src.display(),
            bench_src.display()
        );
        return Ok(());
    }

    std::fs::create_dir_all(&tmp).with_context(|| format!("mkdir {}", tmp.display()))?;
    eprintln!("pgo-train: C smoke/bench/chaos under LD_PRELOAD");
    let smoke_bin = tmp.join("pgo-smoke");
    let bench_bin = tmp.join("pgo-bench");
    compile(&cc, &["-O2", "-pthread", utf8(&smoke_src)?], &smoke_bin)?;
    compile(&cc, &["-O2", "-pthread", utf8(&bench_src)?], &bench_bin)?;
    run_so(&so, &smoke_bin, &[])?;
    run_so(&so, &bench_bin, &[])?;
    if let Some(secure) = secure_so.as_ref().filter(|p| p.is_file()) {
        run_so(secure, &smoke_bin, &[])?;
        run_so(secure, &bench_bin, &[])?;
    }

    let chaos_src = c_tests.join("chaos.c");
    if include.is_dir() && chaos_src.is_file() {
        let chaos_bin = tmp.join("pgo-chaos");
        compile_chaos(&cc, &include, &chaos_src, &so, &chaos_bin)?;
        run_so(
            &so,
            &chaos_bin,
            &[("MIMALLOC_CHAOS_STEPS", chaos_steps("8192"))],
        )?;
        if let Some(secure) = secure_so.as_ref().filter(|p| p.is_file()) {
            let chaos_sec = tmp.join("pgo-chaos-secure");
            compile_chaos(&cc, &include, &chaos_src, secure, &chaos_sec)?;
            run_so(
                secure,
                &chaos_sec,
                &[("MIMALLOC_CHAOS_STEPS", chaos_steps("4096"))],
            )?;
        }
    }
    Ok(())
}

/// Full local pipeline: generate, train, merge, profile-use.
pub fn pgo() -> Result<()> {
    let rust = rust_root();
    let profdata = find_llvm_profdata()?;
    let raw = rust.join("target/pgo-raw");
    let gen = rust.join("target/pgo-gen");
    let gen_secure = rust.join("target/pgo-gen-secure");
    let merged = rust.join("target/pgo.profdata");
    std::fs::create_dir_all(&raw).context("mkdir pgo-raw")?;
    if raw.is_dir() {
        for f in collect_profraw(&raw)? {
            let _ = std::fs::remove_file(f);
        }
    }
    let _ = std::fs::remove_file(&merged);

    let base = std::env::var("RUSTFLAGS").unwrap_or_default();
    let gen_flags = join_rustflags(&base, &format!("-Cprofile-generate={}", raw.display()));
    eprintln!("pgo: instrumented build ({})", profdata.display());
    let gen_s = utf8(&gen)?;
    let gen_secure_s = utf8(&gen_secure)?;
    cargo_pgo(
        &[
            "build",
            "--release",
            "--target-dir",
            gen_s,
            "-p",
            "elymalloc-c",
        ],
        &gen_flags,
    )?;
    cargo_pgo(
        &[
            "build",
            "--release",
            "--target-dir",
            gen_secure_s,
            "-p",
            "elymalloc-c",
            "--features",
            "secure",
        ],
        &gen_flags,
    )?;
    cargo_pgo(
        &[
            "build",
            "--release",
            "--target-dir",
            gen_s,
            "-p",
            "elymalloc-bench",
        ],
        &gen_flags,
    )?;
    cargo_pgo(
        &[
            "build",
            "--release",
            "--target-dir",
            gen_s,
            "-p",
            "elymalloc-alloc-stress",
        ],
        &gen_flags,
    )?;

    let so = release_file(&gen, cdylib_name());
    let secure = release_file(&gen_secure, cdylib_name());
    let bench = release_file(&gen, &bin_name("elymalloc-bench"));
    let stress = release_file(&gen, &bin_name("elymalloc-alloc-stress"));

    std::env::set_var(
        "LLVM_PROFILE_FILE",
        raw.join("elymalloc-%p-%m.profraw").as_os_str(),
    );
    train(TrainArgs {
        so: Some(so),
        secure_so: Some(secure),
        bench: Some(bench),
        stress: Some(stress),
        cc: std::env::var_os("CC").map(PathBuf::from),
        include: Some(repo_root().join("include")),
        c_tests: Some(rust.join("tests")),
        tmp: Some(rust.join("target/pgo-train")),
    })?;
    std::env::remove_var("LLVM_PROFILE_FILE");

    let raws = collect_profraw(&raw)?;
    if raws.is_empty() {
        bail!("pgo: no .profraw produced");
    }
    eprintln!("pgo: merge {} profiles", raws.len());
    let merged_s = utf8(&merged)?.to_string();
    let mut merge_args = vec![
        "merge".to_string(),
        "-sparse".to_string(),
        "-o".to_string(),
        merged_s,
    ];
    for f in &raws {
        merge_args.push(utf8(f)?.to_string());
    }
    let merge_ref: Vec<&str> = merge_args.iter().map(String::as_str).collect();
    run_ok(&profdata, &merge_ref, &[])?;

    let use_flags = join_rustflags(&base, &format!("-Cprofile-use={}", merged.display()));
    eprintln!("pgo: optimized rebuild");
    cargo_pgo(&["build", "--release", "-p", "elymalloc-c"], &use_flags)?;
    cargo_pgo(
        &[
            "build",
            "--release",
            "-p",
            "elymalloc-c",
            "--features",
            "secure",
            "--target-dir",
            "target/mimalloc-secure",
        ],
        &use_flags,
    )?;
    cargo_pgo(&["build", "--release", "-p", "elymalloc-bench"], &use_flags)?;
    eprintln!("pgo: done ({})", merged.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rustc_host_line() {
        let v =
            "rustc 1.80.0 (aaaa 2024-01-01)\nhost: x86_64-unknown-linux-gnu\ncommit-hash: abcd\n";
        assert_eq!(
            host_from_verbose(v).as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert!(host_from_verbose("no host here\n").is_none());
    }

    #[test]
    fn join_rustflags_appends() {
        assert_eq!(
            join_rustflags("", "-Cprofile-generate=/tmp"),
            "-Cprofile-generate=/tmp"
        );
        assert_eq!(
            join_rustflags("-C debuginfo=1", "-Cprofile-use=/x"),
            "-C debuginfo=1 -Cprofile-use=/x"
        );
    }

    #[test]
    fn stack_preload_joins_existing() {
        let so = Path::new("/tmp/libmimalloc.so");
        assert_eq!(
            stack_preload(so, None),
            OsString::from("/tmp/libmimalloc.so")
        );
        let joined = stack_preload(so, Some(OsStr::new("other")));
        #[cfg(windows)]
        assert_eq!(joined, OsString::from("/tmp/libmimalloc.so;other"));
        #[cfg(not(windows))]
        assert_eq!(joined, OsString::from("/tmp/libmimalloc.so:other"));
    }

    #[test]
    fn train_without_so_is_ok() {
        train(TrainArgs::default()).unwrap();
    }

    #[test]
    fn chaos_rpath_points_at_so_dir() {
        let so = Path::new("/build/pgo-gen/release/libmimalloc.so");
        let rpath = format!("-Wl,-rpath,{}", so.parent().unwrap().display());
        assert_eq!(rpath, "-Wl,-rpath,/build/pgo-gen/release");
    }
}
