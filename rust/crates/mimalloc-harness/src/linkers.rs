//! Compile/link under LD_PRELOAD with GNU ld, gold, LLVM LLD, mold, and Wild.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::crash::is_compiler_crash;
use crate::process::{run_captured, write_all};
use crate::rust_root;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linker {
    pub id: &'static str,
    pub rustc: RustcHow,
    pub cc: CcHow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RustcHow {
    /// rustc default (rust-lld on current linux toolchains).
    Default,
    /// `-C linker=cc` plus extra `-C` values (`link-arg=…`, `linker-features=-lld`).
    Cc {
        extra: Vec<String>,
        path_prepend: Option<PathBuf>,
    },
    /// `-C linker=clang` `-C link-arg=--ld-path=…`
    ClangLdPath(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CcHow {
    FuseLd(&'static str),
    FuseLdPath { ld: &'static str, prepend: PathBuf },
    ClangLdPath(PathBuf),
}

pub fn rustc_sysroot() -> Option<PathBuf> {
    let rustc = which::which("rustc").ok()?;
    let out = Command::new(rustc)
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    p.is_dir().then_some(p)
}

pub fn rust_lld_gcc_ld_dir() -> Option<PathBuf> {
    let root = rustc_sysroot()?;
    let rustlib = root.join("lib/rustlib");
    let rd = std::fs::read_dir(&rustlib).ok()?;
    for ent in rd.flatten() {
        let dir = ent.path().join("bin/gcc-ld");
        if dir.join("ld.lld").is_file() {
            return Some(dir);
        }
    }
    None
}

pub fn find_on_path(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

pub fn find_ld_lld() -> Option<PathBuf> {
    if let Some(p) = find_on_path("ld.lld") {
        return Some(p);
    }
    rust_lld_gcc_ld_dir().map(|d| d.join("ld.lld"))
}

pub fn find_wild() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WILD") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    find_on_path("wild")
}

pub fn find_mold() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MOLD") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    find_on_path("mold")
}

pub fn is_elf(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut mag = [0u8; 4];
    matches!(f.read_exact(&mut mag), Ok(())) && mag == *b"\x7fELF"
}

pub fn needed_mentions_mimalloc(readelf_d: &str) -> bool {
    readelf_d.contains("libmimalloc")
}

pub fn wrapper_linker_path(script: &str) -> Option<PathBuf> {
    for line in script.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("linker=") {
            let p = PathBuf::from(rest.trim());
            if !p.as_os_str().is_empty() {
                return Some(p);
            }
        }
    }
    None
}

pub fn resolve_linker_elf(path: &Path) -> PathBuf {
    if is_elf(path) {
        return path.to_path_buf();
    }
    if let Ok(s) = std::fs::read_to_string(path) {
        if let Some(p) = wrapper_linker_path(&s) {
            if is_elf(&p) {
                return p;
            }
        }
    }
    path.to_path_buf()
}

pub fn elf_needed_mimalloc(path: &Path) -> bool {
    let elf = resolve_linker_elf(path);
    let Ok(out) = Command::new("readelf").arg("-d").arg(&elf).output() else {
        return false;
    };
    needed_mentions_mimalloc(&String::from_utf8_lossy(&out.stdout))
}

/// Nixpkgs mold dynamically `DT_NEEDED`s C `libmimalloc-secure`. A mold
/// rebuilt with our static archive has `mi_malloc` in the binary instead.
pub enum VendoredMalloc {
    Dynamic(PathBuf),
    Static(PathBuf),
}

pub fn elf_defines_mi_malloc(path: &Path) -> bool {
    let elf = resolve_linker_elf(path);
    let Ok(out) = Command::new("nm").arg("--defined-only").arg(&elf).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.split_whitespace().last() == Some("mi_malloc"))
}

pub fn mold_vendored_malloc(ln: &Linker) -> Option<VendoredMalloc> {
    if ln.id != "mold" {
        return None;
    }
    let mut cands = Vec::new();
    cands.extend(find_on_path("ld.mold"));
    cands.extend(find_mold());
    for p in cands {
        if elf_needed_mimalloc(&p) {
            return Some(VendoredMalloc::Dynamic(resolve_linker_elf(&p)));
        }
        if elf_defines_mi_malloc(&p) {
            return Some(VendoredMalloc::Static(resolve_linker_elf(&p)));
        }
    }
    None
}

/// Nixpkgs mold is linked to C `libmimalloc-secure`; LD_PRELOAD of a different
/// mimalloc then double-inits and SIGSEGVs in the vendored constructor.
pub fn skip_preload_because_vendored_mimalloc(ln: &Linker) -> Option<PathBuf> {
    match mold_vendored_malloc(ln) {
        Some(VendoredMalloc::Dynamic(p) | VendoredMalloc::Static(p)) => Some(p),
        None => None,
    }
}

pub fn rustc_c_args(how: &RustcHow) -> Vec<String> {
    match how {
        RustcHow::Default => Vec::new(),
        RustcHow::Cc { extra, .. } => {
            let mut v = vec!["linker=cc".into()];
            v.extend(extra.iter().cloned());
            v
        }
        RustcHow::ClangLdPath(p) => vec![
            "linker=clang".into(),
            format!("link-arg=--ld-path={}", p.display()),
        ],
    }
}

/// Linkers to try, in a stable order. Missing tools are omitted.
pub fn discover() -> Vec<Linker> {
    let mut out = Vec::new();
    out.push(Linker {
        id: "rust-lld",
        rustc: RustcHow::Default,
        cc: match rust_lld_gcc_ld_dir() {
            Some(d) => CcHow::FuseLdPath {
                ld: "lld",
                prepend: d,
            },
            None => CcHow::FuseLd("lld"),
        },
    });
    out.push(Linker {
        id: "bfd",
        rustc: RustcHow::Cc {
            extra: vec![
                "linker-features=-lld".into(),
                "link-arg=-fuse-ld=bfd".into(),
            ],
            path_prepend: None,
        },
        cc: CcHow::FuseLd("bfd"),
    });
    if find_on_path("ld.gold").is_some() || find_on_path("gold").is_some() {
        out.push(Linker {
            id: "gold",
            rustc: RustcHow::Cc {
                extra: vec![
                    "linker-features=-lld".into(),
                    "link-arg=-fuse-ld=gold".into(),
                ],
                path_prepend: None,
            },
            cc: CcHow::FuseLd("gold"),
        });
    }
    if let Some(lld) = find_ld_lld() {
        let prepend = lld.parent().unwrap_or(Path::new("/usr/bin")).to_path_buf();
        out.push(Linker {
            id: "lld",
            rustc: RustcHow::Cc {
                extra: vec!["link-arg=-fuse-ld=lld".into()],
                path_prepend: Some(prepend.clone()),
            },
            cc: CcHow::FuseLdPath { ld: "lld", prepend },
        });
    }
    if find_mold().is_some() {
        out.push(Linker {
            id: "mold",
            rustc: RustcHow::Cc {
                extra: vec!["link-arg=-fuse-ld=mold".into()],
                path_prepend: None,
            },
            cc: CcHow::FuseLd("mold"),
        });
    }
    if let Some(wild) = find_wild() {
        out.push(Linker {
            id: "wild",
            rustc: RustcHow::ClangLdPath(wild.clone()),
            cc: CcHow::ClangLdPath(wild),
        });
    }
    out
}

fn apply_linker_path(cmd: &mut Command, how: &RustcHow) {
    if let RustcHow::Cc {
        path_prepend: Some(dir),
        ..
    } = how
    {
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("PATH", path);
    }
}

fn rustc_cmd(how: &RustcHow, src: &Path, out: &Path) -> Command {
    let mut c = Command::new("rustc");
    c.args(["--edition", "2021", "-O"]);
    for a in rustc_c_args(how) {
        c.arg("-C").arg(a);
    }
    apply_linker_path(&mut c, how);
    c.arg("-o").arg(out).arg(src);
    c
}

fn cc_compile_status(how: &CcHow, src: &Path, out: &Path, preload: Option<&Path>) -> Result<i32> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut cmd = match how {
        CcHow::FuseLd(ld) => {
            let mut c = Command::new("gcc");
            c.args(["-O2", "-pthread", &format!("-fuse-ld={ld}")]);
            c
        }
        CcHow::FuseLdPath { ld, prepend } => {
            let mut c = Command::new("gcc");
            let path = format!(
                "{}:{}",
                prepend.display(),
                std::env::var("PATH").unwrap_or_default()
            );
            c.env("PATH", path);
            c.args(["-O2", "-pthread", &format!("-fuse-ld={ld}")]);
            c
        }
        CcHow::ClangLdPath(p) => {
            let clang = which::which("clang").unwrap_or_else(|_| PathBuf::from("clang"));
            let mut c = Command::new(clang);
            c.args(["-O2", "-pthread", &format!("--ld-path={}", p.display())]);
            c
        }
    };
    cmd.arg(src).arg("-o").arg(out);
    if let Some(so) = preload {
        cmd.env("LD_PRELOAD", so);
    }
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let output = cmd.output()?;
    Ok(crate::process::status_code(output.status))
}

pub fn probe_rustc(how: &RustcHow, cache: &Path) -> bool {
    let src = cache.join("link-probe.rs");
    if !src.is_file() {
        let _ = write_all(&src, "fn main() {}\n");
    }
    let out = cache.join("link-probe-rustc");
    let st = rustc_cmd(how, &src, &out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::fs::remove_file(&out);
    matches!(st, Ok(s) if s.success())
}

pub fn probe_cc(how: &CcHow, cache: &Path) -> bool {
    let src = rust_root().join("tests/smoke.c");
    if !src.is_file() {
        return false;
    }
    let out = cache.join("link-probe-cc");
    matches!(cc_compile_status(how, &src, &out, None), Ok(0))
}

pub fn rustc_repeat(
    linker: &Linker,
    so: &Path,
    cache: &Path,
    src: &Path,
    n: u32,
) -> Result<(u32, u32)> {
    let mut ok = 0u32;
    let mut crash = 0u32;
    for i in 1..=n {
        let out = cache.join(format!(
            "r-{}-{}-{i}",
            linker.id,
            so.file_name().unwrap_or_default().to_string_lossy()
        ));
        let mut cmd = rustc_cmd(&linker.rustc, src, &out);
        cmd.env("LD_PRELOAD", so)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let output = cmd.output()?;
        let rc = crate::process::status_code(output.status);
        let err = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            ok += 1;
        } else if is_compiler_crash(rc, &err) || rc != 0 {
            crash += 1;
        }
        let _ = std::fs::remove_file(&out);
    }
    Ok((ok, crash))
}

pub fn cc_repeat(
    linker: &Linker,
    so: &Path,
    cache: &Path,
    src: &Path,
    n: u32,
) -> Result<(u32, u32)> {
    let mut ok = 0u32;
    let mut crash = 0u32;
    for i in 1..=n {
        let out = cache.join(format!("c-{}-{i}", linker.id));
        let rc = cc_compile_status(&linker.cc, src, &out, Some(so))?;
        if rc == 0 {
            ok += 1;
        } else {
            crash += 1;
        }
        let _ = std::fs::remove_file(&out);
    }
    Ok((ok, crash))
}

fn ui_src(cache: &Path) -> PathBuf {
    cache.join("rust-src/tests/ui/unsafe/unsafe-fn-called-from-unsafe-blk.rs")
}

fn repeat_n() -> u32 {
    std::env::var("LINKER_REPEAT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            std::env::var("RUSTC_LLD_REPEAT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(12)
}

fn link_alloc_stress(linker: &Linker, dest: &Path) -> Result<()> {
    let rust = rust_root();
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        std::env::set_var("CARGO_TARGET_DIR", rust.join("target"));
    }
    let mut cmd = crate::process::cargo_in_root();
    cmd.args([
        "rustc",
        "-q",
        "-p",
        "mimalloc-alloc-stress",
        "--release",
        "--bin",
        "mimalloc-alloc-stress",
        "--",
    ]);
    for a in rustc_c_args(&linker.rustc) {
        cmd.arg("-C").arg(a);
    }
    apply_linker_path(&mut cmd, &linker.rustc);
    cmd.env("CARGO_TARGET_DIR", dest);
    let st = cmd.status()?;
    if !st.success() {
        bail!("cargo rustc alloc-stress ({}) failed", linker.id);
    }
    Ok(())
}

fn alloc_stress_bin(dest: &Path) -> PathBuf {
    let p = dest.join("release/mimalloc-alloc-stress");
    if p.is_file() {
        return p;
    }
    dest.join("release/mimalloc-alloc-stress.exe")
}

/// Link `mimalloc-alloc-stress` with each available linker and run it (no preload).
pub fn run_global_alloc_per_linker(cache: &Path) -> Result<()> {
    println!("==> GlobalAlloc binary linked with each linker");
    crate::process::cargo_ok(&["test", "-p", "mimalloc-alloc-stress"])?;
    let linkers = discover();
    let mut ran = 0u32;
    for ln in &linkers {
        if !probe_rustc(&ln.rustc, cache) {
            println!("  skip {} (rustc probe failed)", ln.id);
            continue;
        }
        let dest = cache.join(format!("alloc-stress-{}", ln.id));
        println!("  link alloc-stress ({})", ln.id);
        link_alloc_stress(ln, &dest)?;
        let bin = alloc_stress_bin(&dest);
        if !bin.is_file() {
            bail!("missing {}", bin.display());
        }
        let cap = run_captured(&bin, &[], &[], Duration::from_secs(60))?;
        if cap.rc != 0 {
            bail!(
                "alloc-stress ({}) rc={} stderr={}",
                ln.id,
                cap.rc,
                cap.stderr_str()
            );
        }
        if !cap.stdout_str().contains("alloc-stress ok") {
            bail!("alloc-stress ({}) missing ok line", ln.id);
        }
        println!("  ok   alloc-stress {}", ln.id);
        ran += 1;
    }
    if ran == 0 {
        bail!("no linkers could link alloc-stress");
    }
    Ok(())
}

/// rustc + gcc/clang compile repeats under each `.so`. Fail if Rust crashes and C does not.
pub fn stress_linkers_under_preload(
    rust_so: &Path,
    rust_secure_so: &Path,
    c_so: &Path,
    je_so: &Path,
    cache: &Path,
) -> Result<()> {
    let n = repeat_n();
    let src = ui_src(cache);
    let smoke = rust_root().join("tests/smoke.c");
    let linkers = discover();
    println!("==> linker matrix x{n} (rustc + cc under LD_PRELOAD)");
    if linkers.is_empty() {
        println!("  skip (no linkers discovered)");
        return Ok(());
    }
    let mut rust_crash_extra = false;
    for ln in &linkers {
        let rustc_ok = probe_rustc(&ln.rustc, cache);
        let cc_ok = probe_cc(&ln.cc, cache);
        if !rustc_ok && !cc_ok {
            println!("  skip {} (probe failed)", ln.id);
            continue;
        }
        let rust_preload = match mold_vendored_malloc(ln) {
            Some(VendoredMalloc::Static(_)) => {
                println!(
                    "  skip {} preload (mold statically linked to Rust mimalloc)",
                    ln.id
                );
                continue;
            }
            Some(VendoredMalloc::Dynamic(_)) => {
                if rust_secure_so.is_file() {
                    println!(
                        "  {} rust preload = libmimalloc-secure.so (linker DT_NEEDED mimalloc-secure)",
                        ln.id
                    );
                    rust_secure_so
                } else {
                    println!(
                        "  skip {} preload (linker DT_NEEDED libmimalloc; build --features secure)",
                        ln.id
                    );
                    continue;
                }
            }
            None => rust_so,
        };
        let mut line = format!("  {}", ln.id);
        if rustc_ok && src.is_file() {
            let mut counts = Vec::new();
            for (tag, so) in [("rust", rust_preload), ("c", c_so), ("jemalloc", je_so)] {
                let (ok, crash) = rustc_repeat(ln, so, cache, &src, n)?;
                line.push_str(&format!(" rustc-{tag}={ok}/{n} abort={crash}"));
                counts.push((tag, crash));
            }
            let rust_c = counts[0].1;
            let c_c = counts[1].1;
            if rust_c > 0 && c_c == 0 {
                rust_crash_extra = true;
            }
        }
        if cc_ok && smoke.is_file() {
            for (tag, so) in [("rust", rust_preload), ("c", c_so), ("jemalloc", je_so)] {
                let (ok, crash) = cc_repeat(ln, so, cache, &smoke, n.min(4))?;
                line.push_str(&format!(" cc-{tag}={ok}/{} abort={crash}", n.min(4)));
                if tag == "rust" && crash > 0 {
                    let _ = tag;
                }
            }
        }
        println!("{line}");
        let dir = cache.join("results").join("linkers");
        std::fs::create_dir_all(&dir)?;
        write_all(&dir.join(format!("{}.txt", ln.id)), &line)?;
    }
    if rust_crash_extra {
        bail!("Rust allocator crashed a linker that C mimalloc did not");
    }
    Ok(())
}

pub fn run() -> Result<()> {
    let rust = rust_root();
    let cache = rust.join("target/compiler-stress");
    std::fs::create_dir_all(&cache)?;
    let found: Vec<_> = discover().into_iter().map(|l| l.id).collect();
    println!("linkers: {}", found.join(" "));
    let secure = rust.join("target/release/libmimalloc-secure.so");
    if !secure.is_file() {
        let _ = crate::process::build_mimalloc_cdylibs();
    }
    run_global_alloc_per_linker(&cache)?;

    let so = rust.join("target/release/libmimalloc.so");
    if so.is_file() {
        let c_so = cache.join("c-oracle/libmimalloc-secure.so");
        let je = std::env::var("JEMALLOC_SO").ok().map(PathBuf::from);
        if c_so.is_file() {
            let je_so = je.unwrap_or_else(|| c_so.clone());
            let rust_secure = rust.join("target/release/libmimalloc-secure.so");
            stress_linkers_under_preload(&so, &rust_secure, &c_so, &je_so, &cache)?;
        } else {
            println!("  skip preload linker matrix (no C oracle .so; run oracle)");
        }
    }
    println!("linkers ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustc_default_has_no_c_args() {
        assert!(rustc_c_args(&RustcHow::Default).is_empty());
    }

    #[test]
    fn gnu_ld_disables_rust_lld() {
        let args = rustc_c_args(&RustcHow::Cc {
            extra: vec![
                "linker-features=-lld".into(),
                "link-arg=-fuse-ld=bfd".into(),
            ],
            path_prepend: None,
        });
        assert!(args.iter().any(|a| a.contains("linker=cc")));
        assert!(args.iter().any(|a| a.contains("linker-features=-lld")));
        assert!(args.iter().any(|a| a.contains("fuse-ld=bfd")));
    }

    #[test]
    fn mold_uses_fuse_ld() {
        let args = rustc_c_args(&RustcHow::Cc {
            extra: vec!["link-arg=-fuse-ld=mold".into()],
            path_prepend: None,
        });
        assert!(args.iter().any(|a| a == "link-arg=-fuse-ld=mold"));
    }

    #[test]
    fn wild_uses_clang_ld_path() {
        let p = PathBuf::from("/nix/store/wild/bin/wild");
        let args = rustc_c_args(&RustcHow::ClangLdPath(p));
        assert!(args.iter().any(|a| a == "linker=clang"));
        assert!(args
            .iter()
            .any(|a| a.contains("ld-path=") && a.contains("wild")));
    }

    #[test]
    fn discover_always_has_rust_lld_and_bfd() {
        let ids: Vec<_> = discover().into_iter().map(|l| l.id).collect();
        assert!(ids.contains(&"rust-lld"));
        assert!(ids.contains(&"bfd"));
    }

    #[test]
    fn needed_detects_secure_soname() {
        let d = " 0x0000000000000001 (NEEDED) Shared library: [libmimalloc-secure.so.3]\n";
        assert!(needed_mentions_mimalloc(d));
        assert!(!needed_mentions_mimalloc("NEEDED libc.so.6\n"));
    }

    #[test]
    fn nm_line_is_mi_malloc() {
        assert_eq!(
            "00000000000abc T mi_malloc".split_whitespace().last(),
            Some("mi_malloc")
        );
        assert_ne!(
            "00000000000abc T malloc".split_whitespace().last(),
            Some("mi_malloc")
        );
    }

    #[test]
    fn wrapper_extracts_nix_linker() {
        let s = "linker=/nix/store/xyz-mold-unwrapped-2.42.0/bin/ld.mold\n";
        let p = wrapper_linker_path(s).unwrap();
        assert!(p.ends_with("ld.mold"));
    }
}
