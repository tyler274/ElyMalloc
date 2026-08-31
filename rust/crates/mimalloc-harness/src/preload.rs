//! GCC / Clang / rustc suites: compile once, run under `LD_PRELOAD`, match libc.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::compare::{outputs_match, Captured};
use crate::filter::{
    rustc_ui_include, rustc_ui_list_current, skip_c_torture_source, RUST_UI_LIST_VER,
};
use crate::ids::{rustc_record_name, rustc_test_id, safe_name};
use crate::process::{cargo_in_root, compile, run_captured, run_captured_preload, write_all};
use crate::{env_is_one, repo_root, rust_root};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastMatch {
    Pass,
    Fail,
    Skip,
}

struct Preload {
    so: PathBuf,
    cache: PathBuf,
    bin_root: PathBuf,
    out_root: PathBuf,
    result_dir: PathBuf,
    reuse_bins: bool,
    pass: u32,
    fail: u32,
    skip: u32,
    last: LastMatch,
}

impl Preload {
    fn record(&mut self, status: &str, name: &str) {
        let line = format!("{status} {name}");
        let _ = crate::process::append_line(&self.result_dir.join("all.txt"), &line);
    }

    fn capture_sys(
        &self,
        dest: &Path,
        program: &Path,
        args: &[&str],
        secs: u64,
    ) -> Result<Captured> {
        let rc_path = dest.with_extension("rc");
        if rc_path.is_file() {
            return read_captured(dest);
        }
        let cap = run_captured(program, args, &[], Duration::from_secs(secs))?;
        write_captured(dest, &cap)?;
        Ok(cap)
    }

    fn run_match(
        &mut self,
        name: &str,
        secs: u64,
        program: &Path,
        args: &[&str],
        require_sys: bool,
    ) -> Result<LastMatch> {
        let safe = safe_name(name);
        let base = self.out_root.join("sys").join(&safe);
        let gotp = self.out_root.join("pre").join(&safe);
        let sys = self.capture_sys(&base, program, args, secs)?;
        if sys.rc != 0 {
            if require_sys {
                println!(
                    "  FAIL {name} (does not run with system malloc, rc={})",
                    sys.rc
                );
                self.fail += 1;
                self.record("FAIL", name);
                self.last = LastMatch::Fail;
                return Ok(LastMatch::Fail);
            }
            self.skip += 1;
            self.record("SKIP", name);
            self.last = LastMatch::Skip;
            return Ok(LastMatch::Skip);
        }
        let got = run_captured_preload(&self.so, program, args, Duration::from_secs(secs))?;
        write_captured(&gotp, &got)?;
        if outputs_match(&sys, &got) {
            println!("  ok   {name}");
            self.pass += 1;
            self.record("PASS", name);
            self.last = LastMatch::Pass;
            return Ok(LastMatch::Pass);
        }
        println!("  FAIL {name} (output/rc != system malloc)");
        self.fail += 1;
        self.record("FAIL", name);
        note_mismatch(&self.result_dir.join("mismatch.txt"), name, &sys, &got)?;
        self.last = LastMatch::Fail;
        Ok(LastMatch::Fail)
    }

    fn run_exit(&mut self, name: &str, secs: u64, program: &Path, args: &[&str]) -> Result<()> {
        let safe = safe_name(name);
        let base = self.out_root.join("sys").join(&safe);
        let sys = self.capture_sys(&base, program, args, secs)?;
        if sys.rc != 0 {
            self.skip += 1;
            self.record("SKIP", name);
            return Ok(());
        }
        let got = run_captured_preload(&self.so, program, args, Duration::from_secs(secs))?;
        write_captured(&self.out_root.join("pre").join(&safe), &got)?;
        if sys.rc == got.rc {
            println!("  ok   {name}");
            self.pass += 1;
            self.record("PASS", name);
        } else {
            println!("  FAIL {name} (rc sys={} preload={})", sys.rc, got.rc);
            self.fail += 1;
            self.record("FAIL", name);
            note_mismatch(&self.result_dir.join("mismatch.txt"), name, &sys, &got)?;
        }
        Ok(())
    }

    fn compile_bin(&self, bin: &Path, cc: &Path, args: &[&str]) -> Result<()> {
        if self.reuse_bins && bin.is_file() {
            return Ok(());
        }
        compile(cc, args, bin)
    }
}

fn write_captured(dest: &Path, cap: &Captured) -> Result<()> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(dest.with_extension("stdout"), &cap.stdout)?;
    std::fs::write(dest.with_extension("stderr"), &cap.stderr)?;
    std::fs::write(dest.with_extension("rc"), format!("{}\n", cap.rc))?;
    Ok(())
}

fn read_captured(dest: &Path) -> Result<Captured> {
    Ok(Captured {
        stdout: std::fs::read(dest.with_extension("stdout")).unwrap_or_default(),
        stderr: std::fs::read(dest.with_extension("stderr")).unwrap_or_default(),
        rc: std::fs::read_to_string(dest.with_extension("rc"))
            .unwrap_or_else(|_| "255\n".into())
            .trim()
            .parse()
            .unwrap_or(255),
    })
}

fn note_mismatch(path: &Path, name: &str, sys: &Captured, got: &Captured) -> Result<()> {
    use std::io::Write;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "=== {name}")?;
    writeln!(f, "rc sys={} preload={}", sys.rc, got.rc)?;
    writeln!(
        f,
        "{}",
        similar_diff("stdout", &sys.stdout_str(), &got.stdout_str())
    )?;
    writeln!(
        f,
        "{}",
        similar_diff("stderr", &sys.stderr_str(), &got.stderr_str())
    )?;
    writeln!(f)?;
    Ok(())
}

fn similar_diff(label: &str, a: &str, b: &str) -> String {
    if a == b {
        return String::new();
    }
    format!(
        "--- sys.{label}\n+++ preload.{label}\n- {}\n+ {}",
        a.trim_end(),
        b.trim_end()
    )
}

fn git_sparse(dir: &Path, url: &str, cone: &str, refs: &[&str]) -> Result<()> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    Command::new("git")
        .current_dir(dir)
        .args(["init", "-q"])
        .status()?;
    Command::new("git")
        .current_dir(dir)
        .args(["remote", "add", "origin", url])
        .status()?;
    Command::new("git")
        .current_dir(dir)
        .args(["sparse-checkout", "init", "--cone"])
        .status()?;
    Command::new("git")
        .current_dir(dir)
        .args(["sparse-checkout", "set"])
        .arg(cone)
        .status()?;
    let mut fetched = false;
    for r in refs {
        let st = Command::new("git")
            .current_dir(dir)
            .args(["fetch", "--depth", "1", "origin", r])
            .status()?;
        if st.success() {
            fetched = true;
            break;
        }
    }
    if !fetched {
        bail!("git fetch failed for {url}");
    }
    let st = Command::new("git")
        .current_dir(dir)
        .args(["checkout", "-q", "FETCH_HEAD"])
        .status()?;
    if !st.success() {
        bail!("git checkout failed");
    }
    Ok(())
}

fn fetch_gcc_torture(cache: &Path) -> Result<()> {
    let dest = cache.join("gcc-execute");
    if dir_has_c(&dest) {
        return Ok(());
    }
    println!("==> fetch gcc.c-torture/execute (sparse)");
    let src = cache.join("gcc-src");
    git_sparse(
        &src,
        "https://github.com/gcc-mirror/gcc.git",
        "gcc/testsuite/gcc.c-torture/execute",
        &["master"],
    )?;
    std::fs::create_dir_all(&dest)?;
    std::fs::create_dir_all(cache.join("gcc-ieee"))?;
    let exec = src.join("gcc/testsuite/gcc.c-torture/execute");
    copy_c_files(&exec, &dest)?;
    let ieee = exec.join("ieee");
    if ieee.is_dir() {
        copy_c_files(&ieee, &cache.join("gcc-ieee"))?;
    }
    Ok(())
}

fn fetch_llvm_c(cache: &Path) -> Result<()> {
    let dest = cache.join("llvm-c");
    if dir_has_c(&dest) {
        return Ok(());
    }
    println!("==> fetch llvm-test-suite SingleSource (sparse)");
    let src = cache.join("llvm-src");
    if git_sparse(
        &src,
        "https://github.com/llvm/llvm-test-suite.git",
        "SingleSource/Regression/C",
        &["main", "master"],
    )
    .is_err()
    {
        println!("  skip llvm-test-suite fetch");
        return Ok(());
    }
    std::fs::create_dir_all(&dest)?;
    for f in WalkDir::new(src.join("SingleSource"))
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = f.path();
        if p.extension().and_then(|e| e.to_str()) == Some("c") {
            if let Some(name) = p.file_name() {
                let _ = std::fs::copy(p, dest.join(name));
            }
        }
    }
    Ok(())
}

fn dir_has_c(dir: &Path) -> bool {
    dir.is_dir()
        && WalkDir::new(dir).max_depth(1).into_iter().any(|e| {
            e.ok()
                .is_some_and(|e| e.path().extension().and_then(|x| x.to_str()) == Some("c"))
        })
}

fn copy_c_files(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for ent in std::fs::read_dir(from).with_context(|| format!("read {from:?}"))? {
        let ent = ent?;
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) == Some("c") {
            std::fs::copy(&p, to.join(ent.file_name()))?;
        }
    }
    Ok(())
}

fn fetch_rust_ui(cache: &Path) -> Result<()> {
    let dest = cache.join("rust-ui.list");
    if dest.is_file() {
        if let Ok(text) = std::fs::read_to_string(&dest) {
            if rustc_ui_list_current(&text) {
                return Ok(());
            }
        }
    }
    let ui = cache.join("rust-src/tests/ui");
    if !ui.is_dir() {
        println!("==> fetch rustc tests/ui (sparse, run-pass filter later)");
        if git_sparse(
            &cache.join("rust-src"),
            "https://github.com/rust-lang/rust.git",
            "tests/ui",
            &["main", "master"],
        )
        .is_err()
        {
            println!("  skip rustc ui fetch");
            return Ok(());
        }
    }
    let mut list = format!("# {RUST_UI_LIST_VER}\n");
    for f in WalkDir::new(&ui).into_iter().filter_map(|e| e.ok()) {
        let p = f.path();
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(p).unwrap_or_default();
        if rustc_ui_include(&src) {
            list.push_str(&format!("{}\n", p.display()));
        }
    }
    write_all(&dest, &list)?;
    Ok(())
}

fn run_c_dir(st: &mut Preload, cc: &Path, tag: &str, dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        println!("  skip {tag} (no {})", dir.display());
        return Ok(());
    }
    println!(
        "==> {tag} compile (system cc) + run {} under LD_PRELOAD (match system malloc output)",
        dir.display()
    );
    let bin_dir = st.bin_root.join(tag);
    std::fs::create_dir_all(&bin_dir)?;
    let mut compiled = 0u32;
    let mut ran = 0u32;
    let mut bad = 0u32;
    for ent in std::fs::read_dir(dir)? {
        let ent = ent?;
        let f = ent.path();
        if f.extension().and_then(|e| e.to_str()) != Some("c") {
            continue;
        }
        let name = format!("{tag}:{}", f.file_name().unwrap().to_string_lossy());
        let src = std::fs::read_to_string(&f).unwrap_or_default();
        if skip_c_torture_source(&src) {
            st.skip += 1;
            st.record("SKIP", &name);
            continue;
        }
        let bin = bin_dir.join(f.file_stem().unwrap());
        let args = ["-O2", "-w", "-lm", f.to_str().unwrap()];
        if st.compile_bin(&bin, cc, &args).is_err() {
            st.skip += 1;
            st.record("SKIP", &name);
            continue;
        }
        compiled += 1;
        match st.run_match(&name, 5, &bin, &[], false)? {
            LastMatch::Pass => ran += 1,
            LastMatch::Fail => bad += 1,
            LastMatch::Skip => {}
        }
    }
    println!(
        "  {tag}: compiled={compiled} ran={ran} failed={bad} (PASS requires matching output vs system malloc)"
    );
    Ok(())
}

fn run_rustc_ui(st: &mut Preload, rustc: &Path) -> Result<()> {
    let list = st.cache.join("rust-ui.list");
    println!("==> rustc tests/ui run-pass: system rustc, then LD_PRELOAD run must match output");
    write_all(&st.result_dir.join("rustc-compiled.txt"), "")?;
    if !list.is_file() || std::fs::metadata(&list)?.len() == 0 {
        println!("  skip rustc ui (no tests fetched)");
        return Ok(());
    }
    std::fs::create_dir_all(st.bin_root.join("rustc"))?;
    let mut compiled = 0u32;
    let mut ran = 0u32;
    let mut bad = 0u32;
    let text = std::fs::read_to_string(&list)?;
    let mut compiled_names = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f = PathBuf::from(line);
        let name = rustc_record_name(&f);
        let id = rustc_test_id(&f);
        let bin = st.bin_root.join("rustc").join(&id);
        let args = ["--edition", "2021", "-O", f.to_str().unwrap()];
        if st.compile_bin(&bin, rustc, &args).is_err() {
            st.skip += 1;
            st.record("SKIP", &name);
            continue;
        }
        compiled += 1;
        compiled_names.push(name.clone());
        match st.run_match(&name, 5, &bin, &[], false)? {
            LastMatch::Pass => ran += 1,
            LastMatch::Fail => bad += 1,
            LastMatch::Skip => {}
        }
    }
    compiled_names.sort();
    write_all(
        &st.result_dir.join("rustc-compiled.txt"),
        &(compiled_names.join("\n") + "\n"),
    )?;
    println!(
        "  rustc ui run-pass: compiled={compiled} ran={ran} failed={bad} (output+rc vs system malloc)"
    );
    Ok(())
}

fn find_clang() -> Option<PathBuf> {
    if let Ok(p) = which::which("clang") {
        return Some(p);
    }
    let nix = which::which("nix-build").ok()?;
    let out = Command::new(nix)
        .args(["--no-out-link", "<nixpkgs>", "-A", "clang"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let bin = PathBuf::from(p).join("bin/clang");
    bin.is_file().then_some(bin)
}

pub fn run() -> Result<()> {
    let rust = rust_root();
    let repo = repo_root();
    let cache = rust.join("target/compiler-stress");
    let so = PathBuf::from(std::env::var("SO").unwrap_or_else(|_| {
        rust.join("target/release/libmimalloc.so")
            .to_string_lossy()
            .into()
    }));
    if !env_is_one("SKIP_BUILD") {
        println!("==> build libmimalloc.so");
        let st = cargo_in_root()
            .args(["build", "--release", "-p", "mimalloc-c"])
            .status()?;
        if !st.success() {
            bail!("cargo build mimalloc-c failed");
        }
    }
    let so = so
        .canonicalize()
        .with_context(|| format!("missing {}", so.display()))?;
    let result_dir = PathBuf::from(
        std::env::var("RESULT_DIR")
            .unwrap_or_else(|_| cache.join("results/rust").to_string_lossy().into()),
    );
    std::fs::create_dir_all(&cache)?;
    std::fs::create_dir_all(&result_dir)?;
    write_all(&result_dir.join("all.txt"), "")?;
    write_all(&result_dir.join("mismatch.txt"), "")?;
    let _ = _ulimit_c_zero();

    let gcc = which::which("gcc").ok();
    let gxx = which::which("g++").ok();
    let clang = find_clang();
    let rustc = which::which("rustc").ok();
    println!("gcc:   {}", disp(&gcc));
    println!("g++:   {}", disp(&gxx));
    println!("clang: {}", disp(&clang));
    println!("rustc: {}", disp(&rustc));
    println!("so:    {}", so.display());

    let mut st = Preload {
        so: so.clone(),
        cache: cache.clone(),
        bin_root: PathBuf::from(
            std::env::var("BIN_ROOT")
                .unwrap_or_else(|_| cache.join("runtime-bins").to_string_lossy().into()),
        ),
        out_root: PathBuf::from(
            std::env::var("OUT_ROOT")
                .unwrap_or_else(|_| cache.join("runtime-out").to_string_lossy().into()),
        ),
        result_dir: result_dir.clone(),
        reuse_bins: env_is_one("REUSE_BINS"),
        pass: 0,
        fail: 0,
        skip: 0,
        last: LastMatch::Skip,
    };
    std::fs::create_dir_all(&st.bin_root)?;
    std::fs::create_dir_all(&st.out_root)?;

    let inc = repo.join("include");
    let inc_s = format!("-I{}", inc.display());
    let cc = PathBuf::from("cc");

    if !env_is_one("RUSTC_ONLY") {
        println!("==> host programs under LD_PRELOAD (gcc/g++)");
        let smoke = st.bin_root.join("host/mi-smoke");
        st.compile_bin(
            &smoke,
            &cc,
            &[
                "-O2",
                "-pthread",
                rust.join("tests/smoke.c").to_str().unwrap(),
            ],
        )?;
        st.run_match("gcc-smoke", 60, &smoke, &[], true)?;

        if !env_is_one("SKIP_MI_API") {
            let cxx_bin = st.bin_root.join("host/mi-cxx");
            let _ = std::fs::remove_file(&cxx_bin);
            let gxx = gxx.clone().unwrap_or_else(|| PathBuf::from("c++"));
            let cxx_src = rust.join("tests/cxx.cpp");
            st.compile_bin(
                &cxx_bin,
                &gxx,
                &[
                    "-O2",
                    "-pthread",
                    "-DNDEBUG",
                    &inc_s,
                    cxx_src.to_str().unwrap(),
                    so.to_str().unwrap(),
                ],
            )?;
            st.run_match("gxx-cxx", 60, &cxx_bin, &[], true)?;
        }

        let stress = st.bin_root.join("host/mi-stress");
        let stress_src = repo.join("test/test-stress.c");
        st.compile_bin(
            &stress,
            &cc,
            &[
                "-O2",
                "-pthread",
                "-DUSE_STD_MALLOC",
                "-DNDEBUG",
                &inc_s,
                stress_src.to_str().unwrap(),
            ],
        )?;
        st.run_exit("gcc-stress", 60, &stress, &["2", "4", "2"])?;

        if let Some(clang) = &clang {
            println!("==> host programs under LD_PRELOAD (clang)");
            let smoke_c = st.bin_root.join("host/mi-smoke-clang");
            st.compile_bin(
                &smoke_c,
                clang,
                &[
                    "-O2",
                    "-pthread",
                    rust.join("tests/smoke.c").to_str().unwrap(),
                ],
            )?;
            st.run_match("clang-smoke", 60, &smoke_c, &[], true)?;
            let stress_c = st.bin_root.join("host/mi-stress-clang");
            st.compile_bin(
                &stress_c,
                clang,
                &[
                    "-O2",
                    "-pthread",
                    "-DUSE_STD_MALLOC",
                    "-DNDEBUG",
                    &inc_s,
                    stress_src.to_str().unwrap(),
                ],
            )?;
            st.run_exit("clang-stress", 60, &stress_c, &["2", "4", "2"])?;
        }

        println!("==> rustc: rebuild and test mimalloc-core under LD_PRELOAD");
        let err_path = cache.join("cargo-test.err");
        let mut cmd = cargo_in_root();
        cmd.args(["test", "-p", "mimalloc-core", "--offline"])
            .env("LD_PRELOAD", &so)
            .stderr(std::fs::File::create(&err_path)?);
        let ok = cmd.status()?.success();
        if ok {
            println!("  ok   cargo-test-mimalloc-core");
            st.pass += 1;
            st.record("PASS", "cargo-test-mimalloc-core");
        } else {
            println!("  FAIL cargo-test-mimalloc-core");
            st.fail += 1;
            st.record("FAIL", "cargo-test-mimalloc-core");
        }
    }

    if !env_is_one("RUSTC_ONLY") && !env_is_one("SKIP_C_TORTURE") {
        fetch_gcc_torture(&cache)?;
        fetch_llvm_c(&cache)?;
        if let Some(gcc) = &gcc {
            run_c_dir(&mut st, gcc, "gcc", &cache.join("gcc-execute"))?;
            run_c_dir(&mut st, gcc, "gcc-ieee", &cache.join("gcc-ieee"))?;
        }
        if let Some(clang) = &clang {
            run_c_dir(&mut st, clang, "clang", &cache.join("gcc-execute"))?;
            run_c_dir(&mut st, clang, "clang-ieee", &cache.join("gcc-ieee"))?;
            run_c_dir(&mut st, clang, "llvm-c", &cache.join("llvm-c"))?;
        } else if let Some(gcc) = &gcc {
            run_c_dir(&mut st, gcc, "llvm-c", &cache.join("llvm-c"))?;
        }
    }

    fetch_rust_ui(&cache)?;
    if let Some(rustc) = &rustc {
        run_rustc_ui(&mut st, rustc)?;
    } else {
        write_all(&result_dir.join("rustc-compiled.txt"), "")?;
    }

    split_results(&result_dir)?;
    println!();
    println!(
        "compiler-preload summary: pass={} fail={} skip={} abort=0 so={}",
        st.pass,
        st.fail,
        st.skip,
        so.display()
    );
    println!("results: {}", result_dir.display());
    if st.fail != 0 {
        println!("mismatches: {}", result_dir.join("mismatch.txt").display());
        bail!("compiler-preload had {} failures", st.fail);
    }
    println!("compiler-preload ok");
    Ok(())
}

fn disp(p: &Option<PathBuf>) -> String {
    p.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "missing".into())
}

fn split_results(dir: &Path) -> Result<()> {
    let all = std::fs::read_to_string(dir.join("all.txt")).unwrap_or_default();
    let mut lines: Vec<&str> = all.lines().collect();
    lines.sort();
    write_all(&dir.join("all.txt"), &(lines.join("\n") + "\n"))?;
    let fail: String = lines
        .iter()
        .filter(|l| l.starts_with("FAIL "))
        .map(|l| format!("{l}\n"))
        .collect();
    let pass: String = lines
        .iter()
        .filter(|l| l.starts_with("PASS "))
        .map(|l| format!("{l}\n"))
        .collect();
    let abort: String = lines
        .iter()
        .filter(|l| l.starts_with("ABORT "))
        .map(|l| format!("{l}\n"))
        .collect();
    write_all(&dir.join("fail.txt"), &fail)?;
    write_all(&dir.join("pass.txt"), &pass)?;
    write_all(&dir.join("abort.txt"), &abort)?;
    Ok(())
}

fn _ulimit_c_zero() {
    // Best-effort; ignore errors if the libc crate is unavailable here.
}
