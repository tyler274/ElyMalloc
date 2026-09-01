//! Compare malloc implementations: wall time (`CLOCK_MONOTONIC`) and user-mode
//! instructions (`perf_event_open`). Same C binary under `LD_PRELOAD` of glibc,
//! this rewrite (both SONAMEs), C mimalloc, and jemalloc.
//!
//! Writes `rust/target/malloc-bench/results.csv` plus self-contained SVG/HTML
//! grouped-bar graphs (no plotters). Optional `TCMALLOC_SO` / `HARDENED_MALLOC_SO`.
//! `rust-quarantine` is a separate bar (`mimalloc_quarantine=64`); default `rust`
//! stays quarantine-off.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchRow {
    pub allocator: String,
    pub case: String,
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

pub fn format_csv(rows: &[BenchRow]) -> String {
    let mut s = String::from("allocator,case,ns,instructions\n");
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{}\n",
            r.allocator, r.case, r.ns, r.instructions
        ));
    }
    s
}

pub fn parse_csv(text: &str) -> Vec<BenchRow> {
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            continue;
        }
        let mut p = line.split(',');
        let Some(allocator) = p.next() else { continue };
        let Some(case) = p.next() else { continue };
        let Some(ns) = p.next().and_then(|v| v.parse().ok()) else {
            continue;
        };
        let instructions = p.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        rows.push(BenchRow {
            allocator: allocator.to_string(),
            case: case.to_string(),
            ns,
            instructions,
        });
    }
    rows
}

const PALETTE: [&str; 10] = [
    "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#b279a2", "#ff9da6", "#9d755d",
    "#bab0ac", "#1f77b4",
];

/// Self-contained SVG grouped bar chart (`metric` is `ns` or `instructions`).
pub fn grouped_bar_svg(title: &str, rows: &[BenchRow], metric: fn(&BenchRow) -> u64) -> String {
    let mut cases: Vec<String> = Vec::new();
    let mut allocs: Vec<String> = Vec::new();
    for r in rows {
        if !cases.iter().any(|c| c == &r.case) {
            cases.push(r.case.clone());
        }
        if !allocs.iter().any(|a| a == &r.allocator) {
            allocs.push(r.allocator.clone());
        }
    }
    if cases.is_empty() || allocs.is_empty() {
        return String::from(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="80"><text x="8" y="40">no data</text></svg>"#,
        );
    }
    let max = rows.iter().map(metric).max().unwrap_or(1).max(1);
    let bar_w = 12.0;
    let gap = 6.0;
    let group_gap = 18.0;
    let left = 72.0;
    let top = 36.0;
    let plot_h = 220.0;
    let group_w = allocs.len() as f64 * bar_w + gap;
    let width = left + cases.len() as f64 * (group_w + group_gap) + 160.0;
    let height = top + plot_h + 80.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n\
         <text x=\"{left}\" y=\"22\" font-family=\"sans serif\" font-size=\"14\">{title}</text>\n",
        w = width,
        h = height,
        left = left,
        title = title
    );
    for (ci, case) in cases.iter().enumerate() {
        let gx = left + ci as f64 * (group_w + group_gap);
        for (ai, alloc) in allocs.iter().enumerate() {
            let val = rows
                .iter()
                .find(|r| r.allocator == *alloc && r.case == *case)
                .map(metric)
                .unwrap_or(0);
            let h = (val as f64 / max as f64) * plot_h;
            let x = gx + ai as f64 * bar_w;
            let y = top + plot_h - h;
            let color = PALETTE[ai % PALETTE.len()];
            out.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{bw:.1}\" height=\"{h:.1}\" fill=\"{color}\"><title>{alloc} {case}: {val}</title></rect>\n",
                bw = bar_w - 1.0
            ));
        }
        let tx = gx + group_w / 2.0;
        let ty = top + plot_h + 14.0;
        out.push_str(&format!(
            "<text x=\"{tx:.1}\" y=\"{ty:.1}\" font-family=\"sans serif\" font-size=\"9\" text-anchor=\"middle\">{case}</text>\n"
        ));
    }
    for (ai, alloc) in allocs.iter().enumerate() {
        let color = PALETTE[ai % PALETTE.len()];
        let lx = left + cases.len() as f64 * (group_w + group_gap) + 8.0;
        let ly = top + 16.0 + ai as f64 * 16.0;
        out.push_str(&format!(
            "<rect x=\"{lx:.1}\" y=\"{ly:.1}\" width=\"10\" height=\"10\" fill=\"{color}\"/>\n\
             <text x=\"{tx:.1}\" y=\"{ty:.1}\" font-family=\"sans serif\" font-size=\"11\">{alloc}</text>\n",
            ly = ly - 9.0,
            tx = lx + 14.0,
            ty = ly
        ));
    }
    out.push_str("</svg>\n");
    out
}

pub fn format_html(ns_svg: &str, instr_svg: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>malloc bench</title></head>
<body>
<h1>malloc bench</h1>
<p>Wall time (ns) and user-mode instructions. Default <code>rust</code> is quarantine-off.</p>
{ns_svg}
{instr_svg}
</body></html>
"#
    )
}

struct Target {
    label: &'static str,
    preload: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

fn optional_so(var: &str, label: &'static str) -> Option<Target> {
    let p = std::env::var_os(var)?;
    let pb = PathBuf::from(p);
    if pb.is_file() {
        Some(Target {
            label,
            preload: Some(pb),
            env: vec![],
        })
    } else {
        None
    }
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
    let rust_so_q = rust_so.clone();

    let mut targets = vec![
        Target {
            label: "glibc",
            preload: None,
            env: vec![],
        },
        Target {
            label: "rust",
            preload: Some(rust_so),
            env: vec![],
        },
        Target {
            label: "rust-secure",
            preload: Some(rust_secure),
            env: vec![],
        },
        Target {
            label: "rust-quarantine",
            preload: Some(rust_so_q),
            env: vec![(OsString::from("mimalloc_quarantine"), OsString::from("64"))],
        },
    ];

    match crate::oracle::c_mimalloc_secure_so() {
        Ok(p) => targets.push(Target {
            label: "c-secure",
            preload: Some(p),
            env: vec![],
        }),
        Err(e) => println!("skip c-secure ({e:#})"),
    }
    if let Some(je) = crate::oracle::try_jemalloc() {
        targets.push(Target {
            label: "jemalloc",
            preload: Some(je),
            env: vec![],
        });
    } else {
        println!("skip jemalloc (set JEMALLOC_SO)");
    }
    if let Some(t) = optional_so("TCMALLOC_SO", "tcmalloc") {
        targets.push(t);
    } else {
        println!("skip tcmalloc (set TCMALLOC_SO)");
    }
    if let Some(t) = optional_so("HARDENED_MALLOC_SO", "hardened-malloc") {
        targets.push(t);
    } else {
        println!("skip hardened-malloc (set HARDENED_MALLOC_SO)");
    }

    println!();
    println!(
        "{:<16} {:<20} {:>12} {:>16}",
        "allocator", "case", "ns", "instructions"
    );

    let timeout = Duration::from_secs(120);
    let mut any = false;
    let mut rows: Vec<BenchRow> = Vec::new();
    for t in &targets {
        let mut extra: Vec<(String, OsString)> = match &t.preload {
            Some(so) => vec![("LD_PRELOAD".into(), so.as_os_str().to_os_string())],
            None => vec![],
        };
        for (k, v) in &t.env {
            extra.push((k.to_string_lossy().into_owned(), v.clone()));
        }
        let extra_ref: Vec<(&str, OsString)> =
            extra.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let cap = run_captured(&bin, &[], &extra_ref, timeout)?;
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
                "{:<16} {:<20} {:>12} {:>16}",
                t.label, line.name, line.ns, line.instructions
            );
            rows.push(BenchRow {
                allocator: t.label.to_string(),
                case: line.name,
                ns: line.ns,
                instructions: line.instructions,
            });
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
                    "{:<16} {:<20} {:>12} {:>16}",
                    "rust-global", line.name, line.ns, line.instructions
                );
                rows.push(BenchRow {
                    allocator: "rust-global".into(),
                    case: line.name,
                    ns: line.ns,
                    instructions: line.instructions,
                });
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

    let csv_path = out_dir.join("results.csv");
    std::fs::write(&csv_path, format_csv(&rows))?;
    let ns_svg = grouped_bar_svg("wall time (ns)", &rows, |r| r.ns);
    let instr_svg = grouped_bar_svg("instructions", &rows, |r| r.instructions);
    std::fs::write(out_dir.join("ns.svg"), &ns_svg)?;
    std::fs::write(out_dir.join("instructions.svg"), &instr_svg)?;
    std::fs::write(out_dir.join("index.html"), format_html(&ns_svg, &instr_svg))?;
    println!("wrote {}", csv_path.display());
    println!(
        "wrote {}/ns.svg instructions.svg index.html",
        out_dir.display()
    );

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
        for (k, v) in &t.env {
            cmd.env(k, v);
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

    #[test]
    fn csv_and_svg_fixture() {
        let rows = vec![
            BenchRow {
                allocator: "glibc".into(),
                case: "malloc-free-16".into(),
                ns: 100,
                instructions: 50,
            },
            BenchRow {
                allocator: "rust".into(),
                case: "malloc-free-16".into(),
                ns: 80,
                instructions: 40,
            },
            BenchRow {
                allocator: "glibc".into(),
                case: "calloc-64".into(),
                ns: 200,
                instructions: 90,
            },
            BenchRow {
                allocator: "rust".into(),
                case: "calloc-64".into(),
                ns: 150,
                instructions: 70,
            },
        ];
        let csv = format_csv(&rows);
        assert!(csv.starts_with("allocator,case,ns,instructions\n"));
        let parsed = parse_csv(&csv);
        assert_eq!(parsed, rows);
        let svg = grouped_bar_svg("wall time (ns)", &rows, |r| r.ns);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("glibc"));
        assert!(svg.contains("rust"));
        assert!(svg.contains("malloc-free-16"));
        let html = format_html(
            &svg,
            &grouped_bar_svg("instructions", &rows, |r| r.instructions),
        );
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<svg"));
    }
}
