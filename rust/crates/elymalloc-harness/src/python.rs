//! [CPython](https://github.com/python/cpython) `regrtest` as allocator stress.
//!
//! NixOS `python3` ships a stub `test` package (no `test_list` / …). The
//! harness clones matching `Lib/test` from python/cpython, then **runs**
//! `python3 -m test` under the rewrite, C mimalloc, and libc. Compile/link
//! is not a substitute. Rewrite-only mismatches vs libc FAIL.
//!
//! `PYTHON` / `PYTHON3` select the interpreter. `CPYTHON_SRC=` skips the
//! fetch. `CPYTHON_REFRESH=1` reclones. `CPYTHON_FULL=1` adds slower
//! modules. `CPYTHON_TEST='test_list test_dict'` appends a custom probe.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Result};
use regex::Regex;

use crate::compare::Captured;
use crate::process::{build_mimalloc_cdylibs, check_glibc_cdylib_preload, run_captured_os};
use crate::projects::{git_sparse, run_under_alloc};
use crate::{env_is_one, repo_root, rust_root, which};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Skip,
    FailRewrite,
    FailBoth,
}

struct PySummary {
    result: String,
    tests_run: u32,
    failures: u32,
    skipped: u32,
    files_run: u32,
}

const DEFAULT: &[(&str, &[&str])] = &[
    (
        "python:containers",
        &[
            "test_list",
            "test_dict",
            "test_set",
            "test_tuple",
            "test_collections",
        ],
    ),
    (
        "python:bytes",
        &[
            "test_bytes",
            "test_memoryview",
            "test_array",
            "test_struct",
            "test_mmap",
        ],
    ),
    (
        "python:gc-threads",
        &["test_gc", "test_weakref", "test_threading", "test_queue"],
    ),
    (
        "python:serialize",
        &["test_pickle", "test_copy", "test_hashlib", "test_json"],
    ),
];

const FULL: &[(&str, &[&str])] = &[
    ("python:io", &["test_io", "test_buffer"]),
    (
        "python:numeric",
        &[
            "test_decimal",
            "test_fractions",
            "test_statistics",
            "test_heapq",
            "test_bisect",
        ],
    ),
    ("python:enum-copy", &["test_enum", "test_copy"]),
];

fn cache_dir() -> PathBuf {
    rust_root().join("target/project-suites")
}

fn strip_ansi(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;]*m").unwrap())
        .replace_all(s, "")
        .into_owned()
}

fn suite_text(cap: &Captured) -> String {
    strip_ansi(&format!("{}{}", cap.stdout_str(), cap.stderr_str()))
}

fn parse_u32_commas(s: &str) -> Option<u32> {
    s.replace(',', "").parse().ok()
}

fn parse_py_summary(text: &str) -> Option<PySummary> {
    let text = strip_ansi(text);
    let result = Regex::new(r"(?m)^Result: ([A-Z][A-Z ]*[A-Z]|SUCCESS|FAILURE)\s*$")
        .unwrap()
        .captures(&text)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .or_else(|| {
            Regex::new(r"(?m)^== Tests result: ([A-Z][A-Z ]*) ==")
                .unwrap()
                .captures(&text)
                .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        })?;
    let tests = Regex::new(
        r"(?m)^Total tests: run=([0-9,]+)(?:\s+failures=([0-9,]+))?(?:\s+skipped=([0-9,]+))?",
    )
    .unwrap()
    .captures(&text)?;
    let tests_run = parse_u32_commas(tests.get(1)?.as_str())?;
    let failures = tests
        .get(2)
        .and_then(|m| parse_u32_commas(m.as_str()))
        .unwrap_or(0);
    let skipped = tests
        .get(3)
        .and_then(|m| parse_u32_commas(m.as_str()))
        .unwrap_or(0);
    let files_run = Regex::new(r"(?m)^Total test files: run=([0-9,]+)")
        .unwrap()
        .captures(&text)
        .and_then(|c| parse_u32_commas(c.get(1)?.as_str()))
        .unwrap_or(0);
    if tests_run == 0 && files_run == 0 {
        return None;
    }
    Some(PySummary {
        result,
        tests_run,
        failures,
        skipped,
        files_run,
    })
}

fn summaries_match(a: &Captured, b: &Captured) -> bool {
    if crashed(a) != crashed(b) {
        return false;
    }
    match (
        parse_py_summary(&suite_text(a)),
        parse_py_summary(&suite_text(b)),
    ) {
        (Some(x), Some(y)) => {
            x.result == y.result
                && x.tests_run == y.tests_run
                && x.failures == y.failures
                && x.skipped == y.skipped
                && x.files_run == y.files_run
                && a.rc == b.rc
        }
        _ => false,
    }
}

fn crashed(cap: &Captured) -> bool {
    cap.rc >= 128 || cap.rc == 124
}

fn judge(libc: &Captured, rust: &Captured, c: Option<&Captured>) -> Verdict {
    if crashed(libc) || parse_py_summary(&suite_text(libc)).is_none_or(|s| s.tests_run == 0) {
        return Verdict::Skip;
    }
    if summaries_match(libc, rust) {
        return Verdict::Pass;
    }
    match c {
        Some(c) if !summaries_match(libc, c) => Verdict::FailBoth,
        _ => Verdict::FailRewrite,
    }
}

fn summarize(cap: &Captured) -> String {
    if let Some(s) = parse_py_summary(&suite_text(cap)) {
        return format!(
            "rc={} result={} run={} fail={} skip={} files={}",
            cap.rc, s.result, s.tests_run, s.failures, s.skipped, s.files_run
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

fn python_version(py: &Path) -> Result<String> {
    let cap = run_captured_os(
        py,
        &[
            OsString::from("-c"),
            OsString::from("import sys; print('%d.%d.%d' % sys.version_info[:3])"),
        ],
        &[],
        Duration::from_secs(15),
        None,
        &["LD_PRELOAD"],
    )?;
    if cap.rc != 0 {
        bail!("python version probe failed ({})", summarize(&cap));
    }
    let v = cap.stdout_str().trim().to_string();
    if v.is_empty() {
        bail!("python version probe produced no stdout");
    }
    Ok(v)
}

fn find_python3() -> Result<PathBuf> {
    for key in ["PYTHON3", "PYTHON"] {
        if let Ok(p) = std::env::var(key) {
            let pb = PathBuf::from(&p);
            if pb.is_file() {
                return Ok(pb);
            }
            let cand = pb.join("bin/python3");
            if cand.is_file() {
                return Ok(cand);
            }
        }
    }
    if let Some(p) = which("python3") {
        return Ok(p);
    }
    let repo = repo_root();
    if let Some(nix) = which("nix") {
        let out = Command::new(nix)
            .args(["build", "--no-link", "--print-out-paths", "--inputs-from"])
            .arg(&repo)
            .arg("nixpkgs#python3")
            .output()?;
        if out.status.success() {
            let store = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let py = PathBuf::from(&store).join("bin/python3");
            if py.is_file() {
                return Ok(py);
            }
        }
    }
    if let Some(p) = store_python3() {
        return Ok(p);
    }
    bail!("python3 not found (set PYTHON3 or install nixpkgs python3)");
}

fn store_python3() -> Option<PathBuf> {
    let Ok(rd) = fs::read_dir("/nix/store") else {
        return None;
    };
    let mut cands: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.contains("-python3-3.") || name.contains("-env") || name.contains("debug") {
            continue;
        }
        let p = e.path().join("bin/python3");
        if p.is_file() {
            let rest = name
                .find("-python3-")
                .map(|i| &name[i + "-python3-".len()..])
                .unwrap_or(&name);
            let nums: Vec<u32> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            cands.push((nums, p));
        }
    }
    cands.sort_by(|a, b| b.0.cmp(&a.0));
    cands.into_iter().map(|(_, p)| p).next()
}

fn ensure_cpython_src(py: &Path) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CPYTHON_SRC").or_else(|_| std::env::var("PYTHON_SRC")) {
        let pb = PathBuf::from(p);
        if pb.join("Lib/test/regrtest.py").is_file() && pb.join("Lib/test/test_list.py").is_file() {
            return Ok(pb);
        }
        bail!("CPYTHON_SRC missing Lib/test/regrtest.py or test_list.py");
    }
    let dest = cache_dir().join("cpython");
    if dest.join("Lib/test/test_list.py").is_file() && !env_is_one("CPYTHON_REFRESH") {
        return Ok(dest);
    }
    let ver = python_version(py)?;
    let tag = format!("v{ver}");
    let parts: Vec<&str> = ver.split('.').collect();
    let mm = if parts.len() >= 2 {
        format!("v{}.{}", parts[0], parts[1])
    } else {
        tag.clone()
    };
    println!("==> fetch python/cpython Lib/test ({tag})");
    git_sparse(
        &dest,
        "https://github.com/python/cpython.git",
        &["Lib/test"],
        &[&tag, &ver, &mm, "main"],
    )?;
    if !dest.join("Lib/test/test_list.py").is_file() {
        bail!("cpython checkout missing Lib/test/test_list.py");
    }
    Ok(dest)
}

fn probes() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = DEFAULT
        .iter()
        .map(|(n, t)| {
            (
                (*n).to_string(),
                t.iter().map(|s| (*s).to_string()).collect(),
            )
        })
        .collect();
    if env_is_one("CPYTHON_FULL") {
        for (n, t) in FULL {
            out.push((
                (*n).to_string(),
                t.iter().map(|s| (*s).to_string()).collect(),
            ));
        }
    }
    if let Ok(extra) = std::env::var("CPYTHON_TEST") {
        let tests: Vec<String> = extra.split_whitespace().map(|s| s.to_string()).collect();
        if !tests.is_empty() {
            out.push(("python:custom".into(), tests));
        }
    }
    out
}

fn test_present(src: &Path, name: &str) -> bool {
    let dir = src.join("Lib/test");
    dir.join(format!("{name}.py")).is_file() || dir.join(name).is_dir()
}

fn run_probe(
    py: &Path,
    src: &Path,
    so: Option<&Path>,
    work: &Path,
    tests: &[String],
    timeout: Duration,
) -> Result<Captured> {
    let mut args = vec![
        OsString::from("-m"),
        OsString::from("test"),
        OsString::from("-j1"),
        OsString::from("--timeout=120"),
        OsString::from("-u"),
        OsString::from("all,-network,-urlfetch,-gui,-audio"),
    ];
    for t in tests {
        args.push(OsString::from(t));
    }
    let extra = [(
        OsString::from("PYTHONPATH"),
        src.join("Lib").into_os_string(),
    )];
    run_under_alloc(so, work, py, &args, work, &extra, timeout, py.parent())
}

fn print_verdict(v: Verdict, libc: &Captured, rust: &Captured, c: Option<&Captured>) {
    let c_ok = c.is_some_and(|c| summaries_match(libc, c));
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
    let out = rust_root().join("target/project-suites/python-runs");
    fs::create_dir_all(&out)?;

    let py = find_python3()?;
    let ver = python_version(&py)?;
    let src = ensure_cpython_src(&py)?;
    println!("==> CPython regrtest under NixOS preload injection");
    println!("python3: {} ({ver})", py.display());
    println!("cpython src: {}", src.display());
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
    let timeout = Duration::from_secs(600);

    for (name, tests) in probes() {
        let tests: Vec<String> = tests
            .into_iter()
            .filter(|t| test_present(&src, t))
            .collect();
        if tests.is_empty() {
            println!(".. {name} skip (no matching Lib/test files)");
            skip += 1;
            continue;
        }
        print!(".. {name} libc");
        let libc = run_probe(
            &py,
            &src,
            None,
            &out.join("libc").join(&name),
            &tests,
            timeout,
        )?;
        if crashed(&libc) || parse_py_summary(&suite_text(&libc)).is_none_or(|s| s.tests_run == 0) {
            println!(" skip ({})", summarize(&libc));
            skip += 1;
            continue;
        }
        print!(" rewrite");
        let rust = run_probe(
            &py,
            &src,
            Some(&rust_so),
            &out.join("rewrite").join(&name),
            &tests,
            timeout,
        )?;
        let c = match &c_so {
            Some(so) => {
                print!(" C");
                Some(run_probe(
                    &py,
                    &src,
                    Some(so),
                    &out.join("c").join(&name),
                    &tests,
                    timeout,
                )?)
            }
            None => None,
        };
        let v = judge(&libc, &rust, c.as_ref());
        ran += 1;
        match v {
            Verdict::Pass => pass += 1,
            Verdict::Skip => skip += 1,
            Verdict::FailRewrite => fail_rewrite += 1,
            Verdict::FailBoth => fail_both += 1,
        }
        print_verdict(v, &libc, &rust, c.as_ref());
    }

    println!(
        "python: ran={ran} pass={pass} skip={skip} fail-rewrite={fail_rewrite} fail-both={fail_both}"
    );
    if ran == 0 {
        bail!("no CPython regrtest probes ran");
    }
    if fail_rewrite > 0 {
        bail!("{fail_rewrite} rewrite-only CPython suite failure(s)");
    }
    println!("python-preload: {pass} probes ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ok() -> &'static str {
        "Using random seed: 1\n\
         [1/3] test_list passed\n\
         == Tests result: SUCCESS ==\n\
         All 3 tests OK.\n\
         Total duration: 4.6 sec\n\
         Total tests: run=246 skipped=2\n\
         Total test files: run=3/3\n\
         Result: SUCCESS\n"
    }

    #[test]
    fn regrtest_summary_counts() {
        let s = parse_py_summary(sample_ok()).unwrap();
        assert_eq!(s.result, "SUCCESS");
        assert_eq!(s.tests_run, 246);
        assert_eq!(s.failures, 0);
        assert_eq!(s.skipped, 2);
        assert_eq!(s.files_run, 3);
    }

    #[test]
    fn regrtest_failures_and_commas() {
        let t = "Total tests: run=1,234 failures=2 skipped=10\n\
                 Total test files: run=8/8\n\
                 Result: FAILURE\n";
        let s = parse_py_summary(t).unwrap();
        assert_eq!(s.result, "FAILURE");
        assert_eq!(s.tests_run, 1234);
        assert_eq!(s.failures, 2);
        assert_eq!(s.skipped, 10);
        assert_eq!(s.files_run, 8);
    }

    #[test]
    fn ansi_total_tests_still_parses() {
        let t = "Total tests: run=10 \x1b[31mfailures=1\x1b[0m skipped=0\n\
                 Total test files: run=1/1\n\
                 Result: FAILURE\n";
        let s = parse_py_summary(t).unwrap();
        assert_eq!(s.failures, 1);
        assert_eq!(s.result, "FAILURE");
    }

    #[test]
    fn count_mismatch_is_rewrite_fail() {
        let libc = Captured::from_utf8_lossy_parts(sample_ok(), "", 0);
        let mut bad = sample_ok().to_string();
        bad = bad.replace("run=246", "run=200");
        let rust = Captured::from_utf8_lossy_parts(&bad, "", 0);
        let c = Captured::from_utf8_lossy_parts(sample_ok(), "", 0);
        assert_eq!(judge(&libc, &rust, Some(&c)), Verdict::FailRewrite);
        assert_eq!(judge(&libc, &c, Some(&c)), Verdict::Pass);
    }

    #[test]
    fn libc_crash_skips() {
        let libc = Captured::from_utf8_lossy_parts("", "segfault\n", 139);
        let rust = Captured::from_utf8_lossy_parts(sample_ok(), "", 0);
        assert_eq!(judge(&libc, &rust, None), Verdict::Skip);
    }

    #[test]
    fn default_probes_are_test_modules() {
        for (name, tests) in DEFAULT {
            assert!(name.starts_with("python:"));
            assert!(!tests.is_empty());
            for t in *tests {
                assert!(t.starts_with("test_"));
                assert!(!t.contains('/'));
            }
        }
    }
}
