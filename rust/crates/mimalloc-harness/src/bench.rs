//! Compare malloc implementations: wall time and user-mode instructions.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::process::{build_mimalloc_cdylibs, compile, run_captured};
use crate::{rust_root, which};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchLine {
    pub name: String,
    pub ns: u64,
    pub instructions: u64,
}

pub fn parse_bench_line(line: &str) -> Option<BenchLine> {
    let line = line.trim();
    if !line.starts_with("bench ") {
        return None;
    }
    let rest = &line[6..];
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    let mut ns = None;
    let mut instructions = None;
    for p in parts {
        if let Some(v) = p.strip_prefix("ns=") {
            ns = v.parse().ok();
        } else if let Some(v) = p.strip_prefix("instructions=") {
            instructions = v.parse().ok();
        }
    }
    Some(BenchLine {
        name,
        ns: ns?,
        instructions: instructions.unwrap_or(0),
    })
}

pub fn parse_bench_output(stdout: &[u8]) -> Vec<BenchLine> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(parse_bench_line)
        .collect()
}

struct Target {
    label: &'static str,
    preload: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let rust = rust_root();
    let src = rust.join("tests/bench.c");
    let out_dir = rust.join("target/malloc-bench");
    std::fs::create_dir_all(&out_dir)?;
    let bin = out_dir.join("malloc-bench");
    let cc = std::env::var_os("CC").unwrap_or_else(|| OsString::from("cc"));
    println!("==> compile {}", src.display());
    compile(
        &cc,
        &[src.to_str().unwrap(), "-O2", "-pthread", "-std=c11"],
        &bin,
    )?;

    println!("==> build Rust libmimalloc.so / libmimalloc-secure.so");
    let (rust_so, rust_secure) = build_mimalloc_cdylibs()?;

    let mut targets = vec![
        Target {
            label: "glibc",
            preload: None,
        },
        Target {
            label: "rust",
            preload: Some(rust_so),
        },
        Target {
            label: "rust-secure",
            preload: Some(rust_secure),
        },
    ];

    match crate::oracle::c_mimalloc_secure_so() {
        Ok(p) => targets.push(Target {
            label: "c-secure",
            preload: Some(p),
        }),
        Err(e) => println!("skip c-secure ({e:#})"),
    }
    if let Some(je) = crate::oracle::try_jemalloc() {
        targets.push(Target {
            label: "jemalloc",
            preload: Some(je),
        });
    } else {
        println!("skip jemalloc (set JEMALLOC_SO)");
    }

    println!();
    println!(
        "{:<14} {:<20} {:>12} {:>16}",
        "allocator", "case", "ns", "instructions"
    );

    let timeout = Duration::from_secs(120);
    let mut any = false;
    for t in &targets {
        let extra: Vec<(&str, OsString)> = match &t.preload {
            Some(so) => vec![("LD_PRELOAD", so.as_os_str().to_os_string())],
            None => vec![],
        };
        let cap = run_captured(&bin, &[], &extra, timeout)?;
        if cap.rc != 0 {
            eprintln!(
                "{}: bench exited {} stderr={}",
                t.label,
                cap.rc,
                String::from_utf8_lossy(&cap.stderr)
            );
            continue;
        }
        let lines = parse_bench_output(&cap.stdout);
        if lines.is_empty() {
            eprintln!("{}: no bench lines in stdout", t.label);
            continue;
        }
        any = true;
        for line in lines {
            println!(
                "{:<14} {:<20} {:>12} {:>16}",
                t.label, line.name, line.ns, line.instructions
            );
        }
    }

    let ga = rust.join("target/release/mimalloc-bench");
    println!("==> cargo build -p mimalloc-bench");
    crate::process::cargo_ok(&["build", "--release", "-p", "mimalloc-bench"])?;
    if ga.is_file() {
        let cap = run_captured(&ga, &[], &[], timeout)?;
        if cap.rc == 0 {
            for line in parse_bench_output(&cap.stdout) {
                println!(
                    "{:<14} {:<20} {:>12} {:>16}",
                    "rust-global", line.name, line.ns, line.instructions
                );
                any = true;
            }
        } else {
            eprintln!(
                "mimalloc-bench exited {} stderr={}",
                cap.rc,
                String::from_utf8_lossy(&cap.stderr)
            );
        }
    }

    if crate::env_is_one("HYPERFINE") && which("hyperfine").is_some() {
        println!("\n==> hyperfine (wall clock, one command per allocator)");
        run_hyperfine(&bin, &targets)?;
    }

    if !any {
        bail!("no bench results");
    }
    Ok(())
}

fn run_hyperfine(bin: &Path, targets: &[Target]) -> Result<()> {
    let hyperfine = which("hyperfine").context("hyperfine")?;
    for t in targets {
        let mut cmd = std::process::Command::new(&hyperfine);
        cmd.args(["--warmup", "1", "--style", "none", "-N"]);
        if let Some(so) = &t.preload {
            cmd.env("LD_PRELOAD", so);
        } else {
            cmd.env_remove("LD_PRELOAD");
        }
        cmd.arg(bin);
        println!("-- {}", t.label);
        let st = cmd.status().context("hyperfine")?;
        if !st.success() {
            eprintln!("hyperfine {} failed", t.label);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_one_line() {
        let l = parse_bench_line("bench malloc-free-16 ns=12345 instructions=67890").unwrap();
        assert_eq!(l.name, "malloc-free-16");
        assert_eq!(l.ns, 12345);
        assert_eq!(l.instructions, 67890);
    }

    #[test]
    fn parse_zero_instructions() {
        let l = parse_bench_line("bench calloc-64 ns=1 instructions=0").unwrap();
        assert_eq!(l.instructions, 0);
    }
}
