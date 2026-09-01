//! Bun (`oven-sh/bun`) and Serde (`serde-rs/serde`) test suites.
//!
//! Compile/link success is not enough: each suite must **run** under the
//! rewrite, C mimalloc, and libc. Bun is compared on pass/fail/ran counts
//! (timings and file order are not stable). Serde is compared on stdout,
//! stderr, and exit after stripping durations. Rewrite-only mismatches FAIL.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::browsers::{find_bwrap, preload_bind_dests};
use crate::compare::{outputs_match, Captured};
use crate::normalize::normalize_text;
use crate::process::{build_mimalloc_cdylibs, check_glibc_cdylib_preload, run_captured_os};
use crate::{env_is_one, repo_root, rust_root, which};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Skip,
    FailRewrite,
    FailBoth,
}

struct BunSummary {
    pass: u32,
    fail: u32,
    ran: u32,
}

const BUN_DEFAULT: &[(&str, &[&str])] = &[
    (
        "bun:js-web",
        &[
            "test/js/web/encoding",
            "test/js/web/url",
            "test/js/web/streams",
            "test/js/web/crypto",
        ],
    ),
    (
        "bun:js-node",
        &[
            "test/js/node/buffer",
            "test/js/node/crypto",
            "test/js/node/path",
            "test/js/node/zlib",
        ],
    ),
    (
        "bun:js-bun",
        &[
            "test/js/bun/glob",
            "test/js/bun/sqlite",
            "test/js/bun/stream",
        ],
    ),
];

const BUN_FULL: &[(&str, &[&str])] = &[
    ("bun:js-web-all", &["test/js/web"]),
    ("bun:js-node-all", &["test/js/node"]),
    ("bun:js-bun-all", &["test/js/bun"]),
    ("bun:regression", &["test/regression"]),
];

const BUN_IGNORE: &[&str] = &[
    "**/webview/**",
    "**/third_party/**",
    "**/valkey/**",
    "**/s3/**",
    "**/docker/**",
];

fn want(name: &str) -> bool {
    let sel = std::env::var("PROJECTS").unwrap_or_else(|_| "all".into());
    sel == "all" || sel.split(',').any(|s| s.trim() == name)
}

fn cache_dir() -> PathBuf {
    rust_root().join("target/project-suites")
}

fn companion_so_names(file_name: &str) -> &'static [&'static str] {
    if file_name.contains("secure") {
        &["libmimalloc.so", "libmimalloc.so.3"]
    } else {
        &["libmimalloc-secure.so.3", "libmimalloc-secure.so"]
    }
}

fn preload_sos(so: &Path) -> Result<Vec<PathBuf>> {
    let abs = so
        .canonicalize()
        .with_context(|| format!("canonicalize {}", so.display()))?;
    let mut out = vec![abs.clone()];
    let Some(dir) = abs.parent() else {
        return Ok(out);
    };
    let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    for extra in companion_so_names(name) {
        let p = dir.join(extra);
        if !p.exists() {
            continue;
        }
        let c = p.canonicalize().unwrap_or(p);
        if !out.iter().any(|e| e == &c) {
            out.push(c);
        }
    }
    Ok(out)
}

fn write_preload_file(work: &Path, so: Option<&Path>) -> Result<PathBuf> {
    let path = work.join("ld-nix.so.preload");
    if let Some(so) = so {
        let mut text = String::new();
        for p in preload_sos(so)? {
            text.push_str(&p.display().to_string());
            text.push('\n');
        }
        fs::write(&path, text)?;
    } else {
        fs::write(&path, "")?;
    }
    Ok(path)
}

fn run_under_alloc(
    so: Option<&Path>,
    work: &Path,
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    extra_env: &[(OsString, OsString)],
    timeout: Duration,
    path_prefix: Option<&Path>,
) -> Result<Captured> {
    fs::create_dir_all(work.join("home"))?;
    fs::create_dir_all(work.join("run"))?;
    // Bun's glob path-length tests mkdir trees too deep for Nix `path:` inputs
    // (ENAMETOOLONG while hashing this git tree). Keep TMPDIR off the repo.
    let tmp = std::env::temp_dir().join("mimalloc-projects").join(
        work.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("run")),
    );
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let preload_file = write_preload_file(work, so)?;
    let mut env = extra_env.to_vec();
    env.push((OsString::from("HOME"), work.join("home").into_os_string()));
    env.push((OsString::from("TMPDIR"), tmp.into_os_string()));
    env.push((
        OsString::from("XDG_RUNTIME_DIR"),
        work.join("run").into_os_string(),
    ));
    if let Ok(tc) = std::env::var("RUSTUP_TOOLCHAIN") {
        env.push((OsString::from("RUSTUP_TOOLCHAIN"), OsString::from(tc)));
    }
    if let Ok(rh) = std::env::var("RUSTUP_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.rustup")))
    {
        env.push((OsString::from("RUSTUP_HOME"), OsString::from(rh)));
    }
    if let Ok(home) = std::env::var("CARGO_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.cargo")))
    {
        env.push((OsString::from("CARGO_HOME"), OsString::from(home)));
    }
    if let Some(dir) = path_prefix {
        let mut path = dir.as_os_str().to_os_string();
        path.push(":");
        if let Ok(p) = std::env::var("PATH") {
            path.push(p);
        }
        env.push((OsString::from("PATH"), path));
    }
    let remove = ["LD_PRELOAD", "WAYLAND_DISPLAY", "DISPLAY"];
    if let Some(bwrap) = find_bwrap() {
        let dests = preload_bind_dests();
        let mut argv: Vec<OsString> = vec![
            "--bind".into(),
            "/".into(),
            "/".into(),
            "--dev-bind".into(),
            "/dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--die-with-parent".into(),
            "--chdir".into(),
            cwd.as_os_str().to_os_string(),
        ];
        for d in dests {
            argv.push("--ro-bind".into());
            argv.push(preload_file.clone().into());
            argv.push(d.into());
        }
        argv.push(program.as_os_str().to_os_string());
        argv.extend(args.iter().cloned());
        return run_captured_os(&bwrap, &argv, &env, timeout, Some(work), &remove);
    }
    if let Some(so) = so {
        let mut joined = OsString::new();
        for (i, p) in preload_sos(so)?.into_iter().enumerate() {
            if i > 0 {
                joined.push(":");
            }
            joined.push(p);
        }
        env.push((OsString::from("LD_PRELOAD"), joined));
    }
    run_captured_os(program, args, &env, timeout, Some(cwd), &remove)
}

fn git_sparse(dir: &Path, url: &str, cones: &[&str], refs: &[&str]) -> Result<()> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
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
    let mut set = Command::new("git");
    set.current_dir(dir)
        .args(["sparse-checkout", "set"])
        .args(cones);
    let st = set.status()?;
    if !st.success() {
        bail!("git sparse-checkout set failed for {url}");
    }
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
        bail!("git checkout failed for {url}");
    }
    Ok(())
}

pub(crate) fn git_clone_depth(dir: &Path, url: &str, refs: &[&str]) -> Result<()> {
    let _ = fs::remove_dir_all(dir);
    let parent = dir.parent().context("clone parent")?;
    fs::create_dir_all(parent)?;
    for r in refs {
        let st = Command::new("git")
            .args(["clone", "--depth", "1", "--branch", r, url])
            .arg(dir)
            .status()?;
        if st.success() {
            return Ok(());
        }
        let _ = fs::remove_dir_all(dir);
    }
    let st = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(dir)
        .status()?;
    if st.success() {
        return Ok(());
    }
    bail!("git clone failed for {url}");
}

fn crashed(cap: &Captured) -> bool {
    cap.rc >= 128 || cap.rc == 124
}

fn suite_text(cap: &Captured) -> String {
    format!("{}{}", cap.stdout_str(), cap.stderr_str())
}

fn re_duration() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            \s*\[[0-9]+(?:\.[0-9]+)?m?s\]
            | finished\ in\ [0-9]+(?:\.[0-9]+)?s
            | \(\s*[0-9]+(?:\.[0-9]+)?s\s*\)
        ",
        )
        .unwrap()
    })
}

/// Strip timings so bun/cargo suite output can be compared across allocators.
pub fn normalize_suite(s: &str) -> String {
    let s = normalize_text(s);
    re_duration().replace_all(&s, "").into_owned()
}

fn parse_bun_summary(text: &str) -> Option<BunSummary> {
    let pass = Regex::new(r"(?m)^ *(\d+) pass")
        .unwrap()
        .captures(text)
        .and_then(|c| c.get(1)?.as_str().parse().ok())?;
    let fail = Regex::new(r"(?m)^ *(\d+) fail")
        .unwrap()
        .captures(text)
        .and_then(|c| c.get(1)?.as_str().parse().ok())
        .unwrap_or(0);
    let ran = Regex::new(r"Ran (\d+) tests")
        .unwrap()
        .captures(text)
        .and_then(|c| c.get(1)?.as_str().parse().ok())?;
    Some(BunSummary { pass, fail, ran })
}

fn bun_summaries_match(a: &Captured, b: &Captured) -> bool {
    if crashed(a) != crashed(b) {
        return false;
    }
    match (
        parse_bun_summary(&suite_text(a)),
        parse_bun_summary(&suite_text(b)),
    ) {
        (Some(x), Some(y)) => x.pass == y.pass && x.fail == y.fail && x.ran == y.ran,
        _ => outputs_match(a, b),
    }
}

fn cargo_fingerprint(cap: &Captured) -> Option<String> {
    let text = suite_text(cap);
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("test result:") || line.starts_with("running ") {
            lines.push(re_duration().replace_all(line, "").into_owned());
        }
    }
    if lines.iter().any(|l| l.starts_with("test result:")) {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn serde_match(a: &Captured, b: &Captured) -> bool {
    if a.rc != b.rc {
        return false;
    }
    match (cargo_fingerprint(a), cargo_fingerprint(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn judge_bun(libc: &Captured, rust: &Captured, c: Option<&Captured>) -> Verdict {
    if crashed(libc) || parse_bun_summary(&suite_text(libc)).is_none_or(|s| s.ran == 0) {
        return Verdict::Skip;
    }
    if bun_summaries_match(libc, rust) {
        return Verdict::Pass;
    }
    match c {
        Some(c) if !bun_summaries_match(libc, c) => Verdict::FailBoth,
        _ => Verdict::FailRewrite,
    }
}

fn judge_serde(libc: &Captured, rust: &Captured, c: Option<&Captured>) -> Verdict {
    if crashed(libc) || cargo_fingerprint(libc).is_none() {
        return Verdict::Skip;
    }
    if serde_match(libc, rust) {
        return Verdict::Pass;
    }
    match c {
        Some(c) if !serde_match(libc, c) => Verdict::FailBoth,
        _ => Verdict::FailRewrite,
    }
}

fn summarize(cap: &Captured) -> String {
    let text = suite_text(cap);
    if let Some(s) = parse_bun_summary(&text) {
        return format!(
            "rc={} pass={} fail={} ran={}",
            cap.rc, s.pass, s.fail, s.ran
        );
    }
    let err = cap.stderr_str();
    let err = err.lines().next().unwrap_or("").trim();
    if err.is_empty() {
        format!("rc={}", cap.rc)
    } else {
        format!(
            "rc={} err={}",
            cap.rc,
            err.chars().take(80).collect::<String>()
        )
    }
}

fn find_bun() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("BUN") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        }
        let cand = pb.join("bin/bun");
        if cand.is_file() {
            return Ok(cand);
        }
    }
    if let Some(p) = which("bun") {
        return Ok(p);
    }
    let repo = repo_root();
    if let Some(nix) = which("nix") {
        let out = Command::new(nix)
            .args(["build", "--no-link", "--print-out-paths", "--inputs-from"])
            .arg(&repo)
            .arg("nixpkgs#bun")
            .output()?;
        if out.status.success() {
            let store = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let bun = PathBuf::from(&store).join("bin/bun");
            if bun.is_file() {
                return Ok(bun);
            }
        }
    }
    bail!("bun not found (set BUN or install nixpkgs bun)");
}

fn bun_version(bun: &Path) -> Result<String> {
    let out = Command::new(bun).arg("--version").output()?;
    if !out.status.success() {
        bail!("bun --version failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn ensure_bun_src(bun: &Path) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("BUN_SRC") {
        let pb = PathBuf::from(p);
        if pb.join("test/harness.ts").is_file() {
            return Ok(pb);
        }
        bail!("BUN_SRC missing test/harness.ts");
    }
    let dest = cache_dir().join("bun");
    if dest.join("test/harness.ts").is_file() && !env_is_one("BUN_REFRESH") {
        return Ok(dest);
    }
    let ver = bun_version(bun)?;
    let tag = format!("bun-v{ver}");
    println!("==> fetch oven-sh/bun tests ({tag})");
    git_sparse(
        &dest,
        "https://github.com/oven-sh/bun.git",
        &["test", "bunfig.toml", "package.json"],
        &[&tag, "main"],
    )?;
    if !dest.join("test/harness.ts").is_file() {
        bail!("bun checkout missing test/harness.ts");
    }
    Ok(dest)
}

fn ensure_serde_src() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("SERDE_SRC") {
        let pb = PathBuf::from(p);
        if pb.join("Cargo.toml").is_file() {
            return Ok(pb);
        }
        bail!("SERDE_SRC missing Cargo.toml");
    }
    let dest = cache_dir().join("serde");
    if dest.join("Cargo.toml").is_file()
        && dest.join("test_suite").is_dir()
        && !env_is_one("SERDE_REFRESH")
    {
        return Ok(dest);
    }
    println!("==> fetch serde-rs/serde");
    git_clone_depth(
        &dest,
        "https://github.com/serde-rs/serde.git",
        &["master", "main"],
    )?;
    Ok(dest)
}

fn bun_probes() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut out: Vec<(&'static str, Vec<&'static str>)> =
        BUN_DEFAULT.iter().map(|(n, p)| (*n, p.to_vec())).collect();
    if env_is_one("BUN_FULL") {
        for (n, p) in BUN_FULL {
            out.push((*n, p.to_vec()));
        }
    }
    if let Ok(extra) = std::env::var("BUN_TEST") {
        let paths: Vec<&'static str> = extra
            .split_whitespace()
            .map(|s| Box::leak(s.to_string().into_boxed_str()) as &str)
            .collect();
        if !paths.is_empty() {
            out.push(("bun:custom", paths));
        }
    }
    out
}

fn run_bun_probe(
    bun: &Path,
    src: &Path,
    so: Option<&Path>,
    work: &Path,
    paths: &[&str],
) -> Result<Captured> {
    let mut args = vec![OsString::from("test"), OsString::from("--timeout=15000")];
    for ign in BUN_IGNORE {
        args.push(OsString::from("--path-ignore-patterns"));
        args.push(OsString::from(*ign));
    }
    for p in paths {
        args.push(OsString::from(*p));
    }
    run_under_alloc(
        so,
        work,
        bun,
        &args,
        src,
        &[],
        Duration::from_secs(180),
        bun.parent(),
    )
}

fn cargo_bin() -> PathBuf {
    which("cargo").unwrap_or_else(|| PathBuf::from("cargo"))
}

fn prepare_serde(src: &Path) -> Result<PathBuf> {
    let target = cache_dir().join("serde-target");
    fs::create_dir_all(&target)?;
    println!("==> cargo fetch / test --no-run serde workspace");
    let st = Command::new(cargo_bin())
        .current_dir(src)
        .env("CARGO_TARGET_DIR", &target)
        .env(
            "RUSTUP_TOOLCHAIN",
            std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into()),
        )
        .args(["fetch"])
        .status()?;
    if !st.success() {
        bail!("cargo fetch serde failed");
    }
    let st = Command::new(cargo_bin())
        .current_dir(src)
        .env("CARGO_TARGET_DIR", &target)
        .env(
            "RUSTUP_TOOLCHAIN",
            std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".into()),
        )
        .args([
            "test",
            "--workspace",
            "--all-targets",
            "--offline",
            "--no-run",
        ])
        .status()?;
    if !st.success() {
        bail!("cargo test --no-run serde failed");
    }
    Ok(target)
}

fn run_serde_probe(src: &Path, target: &Path, so: Option<&Path>, work: &Path) -> Result<Captured> {
    let cargo = cargo_bin();
    let args: Vec<OsString> = [
        "test",
        "--workspace",
        "--all-targets",
        "--offline",
        "--",
        "--test-threads=1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let extra = [(
        OsString::from("CARGO_TARGET_DIR"),
        target.as_os_str().to_os_string(),
    )];
    run_under_alloc(
        so,
        work,
        &cargo,
        &args,
        src,
        &extra,
        Duration::from_secs(180),
        cargo.parent(),
    )
}

fn print_verdict(v: Verdict, libc: &Captured, rust: &Captured, c: Option<&Captured>, bun: bool) {
    let c_ok = c.is_some_and(|c| {
        if bun {
            bun_summaries_match(libc, c)
        } else {
            serde_match(libc, c)
        }
    });
    match v {
        Verdict::Pass => {
            if c.is_some() {
                if c_ok {
                    println!(" ok (C matched)");
                } else {
                    println!(" ok (C mismatched libc; rewrite matched)");
                }
            } else {
                println!(" ok");
            }
        }
        Verdict::Skip => println!(" skip ({})", summarize(libc)),
        Verdict::FailRewrite => {
            println!(" FAIL rewrite-only ({})", summarize(rust));
        }
        Verdict::FailBoth => println!(" note C also mismatched ({})", summarize(rust)),
    }
}

pub fn run() -> Result<()> {
    let (rust_so, _) = build_mimalloc_cdylibs()?;
    check_glibc_cdylib_preload(&rust_so)?;
    let c_so = crate::oracle::c_mimalloc_secure_so().ok();
    let out = rust_root().join("target/project-suites/runs");
    fs::create_dir_all(&out)?;

    println!("==> project test suites under NixOS preload injection");
    println!("rewrite so: {}", rust_so.display());
    match &c_so {
        Some(p) => println!("C so:      {}", p.display()),
        None => println!("C so:      (unavailable; rewrite must still match libc)"),
    }

    let mut ran = 0usize;
    let mut pass = 0usize;
    let mut skip = 0usize;
    let mut fail_rewrite = 0usize;
    let mut fail_both = 0usize;

    if want("bun") {
        let bun = find_bun()?;
        let src = ensure_bun_src(&bun)?;
        println!("bun: {} ({})", bun.display(), bun_version(&bun)?);
        println!("bun src: {}", src.display());
        for (name, paths) in bun_probes() {
            print!(".. {name} libc");
            let libc = run_bun_probe(&bun, &src, None, &out.join("libc").join(name), &paths)?;
            if crashed(&libc) || parse_bun_summary(&suite_text(&libc)).is_none_or(|s| s.ran == 0) {
                println!(" skip ({})", summarize(&libc));
                skip += 1;
                continue;
            }
            print!(" rewrite");
            let rust = run_bun_probe(
                &bun,
                &src,
                Some(&rust_so),
                &out.join("rewrite").join(name),
                &paths,
            )?;
            let c = match &c_so {
                Some(so) => {
                    print!(" C");
                    Some(run_bun_probe(
                        &bun,
                        &src,
                        Some(so),
                        &out.join("c").join(name),
                        &paths,
                    )?)
                }
                None => None,
            };
            let v = judge_bun(&libc, &rust, c.as_ref());
            ran += 1;
            match v {
                Verdict::Pass => pass += 1,
                Verdict::Skip => skip += 1,
                Verdict::FailRewrite => fail_rewrite += 1,
                Verdict::FailBoth => fail_both += 1,
            }
            print_verdict(v, &libc, &rust, c.as_ref(), true);
        }
    }

    if want("serde") {
        let src = ensure_serde_src()?;
        let target = prepare_serde(&src)?;
        println!("serde src: {}", src.display());
        let name = "serde:workspace";
        print!(".. {name} libc");
        let libc = run_serde_probe(&src, &target, None, &out.join("libc").join(name))?;
        if crashed(&libc) || cargo_fingerprint(&libc).is_none() {
            println!(" skip ({})", summarize(&libc));
            skip += 1;
        } else {
            print!(" rewrite");
            let rust = run_serde_probe(
                &src,
                &target,
                Some(&rust_so),
                &out.join("rewrite").join(name),
            )?;
            let c = match &c_so {
                Some(so) => {
                    print!(" C");
                    Some(run_serde_probe(
                        &src,
                        &target,
                        Some(so),
                        &out.join("c").join(name),
                    )?)
                }
                None => None,
            };
            let v = judge_serde(&libc, &rust, c.as_ref());
            ran += 1;
            match v {
                Verdict::Pass => pass += 1,
                Verdict::Skip => skip += 1,
                Verdict::FailRewrite => fail_rewrite += 1,
                Verdict::FailBoth => fail_both += 1,
            }
            print_verdict(v, &libc, &rust, c.as_ref(), false);
        }
    }

    println!(
        "projects: ran={ran} pass={pass} skip={skip} fail-rewrite={fail_rewrite} fail-both={fail_both}"
    );
    if ran == 0 {
        bail!("no bun/serde project probes ran");
    }
    if fail_rewrite > 0 {
        bail!("{fail_rewrite} rewrite-only project suite failure(s)");
    }
    println!("projects-preload: {pass} probes ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bun_summary_counts() {
        let t = " 2386 pass\n 1 fail\n 1 snapshots, 115847 expect() calls\nRan 2387 tests across 8 files. [525.00ms]\n";
        let s = parse_bun_summary(t).unwrap();
        assert_eq!(s.pass, 2386);
        assert_eq!(s.fail, 1);
        assert_eq!(s.ran, 2387);
    }

    #[test]
    fn suite_times_stripped() {
        let a = normalize_suite("Ran 10 tests across 2 files. [1.86s]\n(pass) foo [3.00ms]\n");
        let b = normalize_suite("Ran 10 tests across 2 files. [9.00s]\n(pass) foo [12.00ms]\n");
        assert_eq!(a, b);
        let c = normalize_suite("test result: ok. 2 passed; 0 failed; finished in 0.01s\n");
        let d = normalize_suite("test result: ok. 2 passed; 0 failed; finished in 4.52s\n");
        assert_eq!(c, d);
    }

    #[test]
    fn bun_default_probes_are_upstream_paths() {
        for (name, paths) in BUN_DEFAULT {
            assert!(name.starts_with("bun:"));
            assert!(!paths.is_empty());
            for p in *paths {
                assert!(p.starts_with("test/"));
            }
        }
    }

    #[test]
    fn libc_crash_skips_bun() {
        let libc = Captured::from_utf8_lossy_parts("", "segfault\n", 139);
        let rust = Captured::from_utf8_lossy_parts(
            " 1 pass\n 0 fail\nRan 1 tests across 1 files.\n",
            "",
            0,
        );
        assert_eq!(judge_bun(&libc, &rust, None), Verdict::Skip);
    }

    #[test]
    fn serde_fingerprint_ignores_cargo_timing() {
        let a = Captured::from_utf8_lossy_parts(
            "running 2 tests\ntest foo ... ok\ntest result: ok. 2 passed; 0 failed; finished in 0.01s\n",
            "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s\n",
            0,
        );
        let b = Captured::from_utf8_lossy_parts(
            "running 2 tests\ntest foo ... ok\ntest result: ok. 2 passed; 0 failed; finished in 4.52s\n",
            "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s\n   Compiling serde v1.0.0\n",
            0,
        );
        assert!(serde_match(&a, &b));
        let c = Captured::from_utf8_lossy_parts(
            "running 2 tests\ntest result: FAILED. 1 passed; 1 failed; finished in 0.01s\n",
            "",
            101,
        );
        assert!(!serde_match(&a, &c));
    }

    #[test]
    fn bun_count_mismatch_is_rewrite_fail() {
        let libc = Captured::from_utf8_lossy_parts(
            " 10 pass\n 0 fail\nRan 10 tests across 1 files.\n",
            "",
            0,
        );
        let rust = Captured::from_utf8_lossy_parts(
            " 8 pass\n 2 fail\nRan 10 tests across 1 files.\n",
            "",
            1,
        );
        let c = Captured::from_utf8_lossy_parts(
            " 10 pass\n 0 fail\nRan 10 tests across 1 files.\n",
            "",
            0,
        );
        assert_eq!(judge_bun(&libc, &rust, Some(&c)), Verdict::FailRewrite);
        assert_eq!(judge_bun(&libc, &c, Some(&c)), Verdict::Pass);
    }
}
