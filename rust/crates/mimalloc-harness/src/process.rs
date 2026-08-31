use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use wait_timeout::ChildExt;

use crate::compare::Captured;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

pub fn status_code(status: ExitStatus) -> i32 {
    if let Some(c) = status.code() {
        return c;
    }
    #[cfg(unix)]
    if let Some(sig) = status.signal() {
        return 128 + sig;
    }
    255
}

pub fn run_captured(
    program: impl AsRef<OsStr>,
    args: &[&str],
    extra_env: &[(&str, OsString)],
    timeout: Duration,
) -> Result<Captured> {
    let args_os: Vec<OsString> = args.iter().map(OsString::from).collect();
    let env_os: Vec<(OsString, OsString)> = extra_env
        .iter()
        .map(|(k, v)| (OsString::from(*k), v.clone()))
        .collect();
    run_captured_os(program, &args_os, &env_os, timeout, None, &[])
}

/// Like [`run_captured`], with cwd, extra env-removes, and `OsString` argv.
pub fn run_captured_os(
    program: impl AsRef<OsStr>,
    args: &[OsString],
    extra_env: &[(OsString, OsString)],
    timeout: Duration,
    current_dir: Option<&Path>,
    remove_env: &[&str],
) -> Result<Captured> {
    let mut cmd = Command::new(program.as_ref());
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    for k in remove_env {
        cmd.env_remove(k);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn {:?}", program.as_ref()))?;
    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let t_out = thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).ok();
        b
    });
    let t_err = thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).ok();
        b
    });
    let status = match child.wait_timeout(timeout).context("wait")? {
        Some(st) => st,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = t_out.join().unwrap_or_default();
            let stderr = t_err.join().unwrap_or_default();
            return Ok(Captured {
                stdout,
                stderr,
                rc: 124,
            });
        }
    };
    Ok(Captured {
        stdout: t_out.join().unwrap_or_default(),
        stderr: t_err.join().unwrap_or_default(),
        rc: status_code(status),
    })
}

pub fn run_captured_preload(
    so: &Path,
    program: impl AsRef<OsStr>,
    args: &[&str],
    timeout: Duration,
) -> Result<Captured> {
    run_captured(
        program,
        args,
        &[("LD_PRELOAD", so.as_os_str().to_os_string())],
        timeout,
    )
}

pub fn compile(cc: impl AsRef<OsStr>, args: &[&str], out: &Path) -> Result<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut cmd = Command::new(cc.as_ref());
    cmd.args(args).arg("-o").arg(out);
    let st = cmd.status().with_context(|| format!("compile {out:?}"))?;
    if !st.success() {
        bail!("compile failed: {out:?}");
    }
    Ok(())
}

pub fn cargo_in_root() -> Command {
    if std::env::var_os("RUSTUP_TOOLCHAIN").is_none() {
        std::env::set_var("RUSTUP_TOOLCHAIN", "stable");
    }
    let mut c = Command::new("cargo");
    c.current_dir(crate::rust_root());
    c
}

pub fn cargo_ok(args: &[&str]) -> Result<()> {
    let st = cargo_in_root().args(args).status()?;
    if !st.success() {
        bail!("cargo {args:?} failed");
    }
    Ok(())
}

pub const DEFAULT_SONAME: &str = "libmimalloc.so.3";
pub const SECURE_SONAME: &str = "libmimalloc-secure.so.3";

/// C `MI_SECURE` names the library `mimalloc-secure`; cargo emits `libmimalloc.so`
/// unless we copy it. Match on the file name or the `--target-dir` used for the
/// secure build.
pub fn expected_soname(so: &Path) -> &'static str {
    let path = so.to_string_lossy();
    let fname = so
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if fname.contains("secure") || path.contains("mimalloc-secure") {
        SECURE_SONAME
    } else {
        DEFAULT_SONAME
    }
}

pub fn parse_dt_soname(readelf_d: &str) -> Option<String> {
    const MARK: &str = "Library soname: [";
    for line in readelf_d.lines() {
        if let Some(i) = line.find(MARK) {
            let rest = &line[i + MARK.len()..];
            if let Some(end) = rest.find(']') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

pub fn readelf_soname(so: &Path) -> Result<String> {
    let readelf = which::which("readelf").context("readelf")?;
    let out = Command::new(readelf).arg("-d").arg(so).output()?;
    parse_dt_soname(&String::from_utf8_lossy(&out.stdout))
        .with_context(|| format!("no SONAME in {}", so.display()))
}

fn find_release_cdylib(target_dir: &Path) -> Result<PathBuf> {
    let direct = target_dir.join("release/libmimalloc.so");
    if direct.is_file() {
        return Ok(direct);
    }
    for e in walkdir::WalkDir::new(target_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_name() == "libmimalloc.so"
            && e.path()
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "release")
        {
            return Ok(e.into_path());
        }
    }
    bail!("libmimalloc.so not found under {}", target_dir.display());
}

/// Default `libmimalloc.so` plus `libmimalloc-secure.so` (SONAME `libmimalloc-secure.so.3`).
pub fn build_mimalloc_cdylibs() -> Result<(PathBuf, PathBuf)> {
    cargo_ok(&["build", "--release", "-p", "mimalloc-c"])?;
    let rust = crate::rust_root();
    let default = find_release_cdylib(&rust.join("target"))?;
    let secure_dir = rust.join("target/mimalloc-secure");
    let st = cargo_in_root()
        .args([
            "build",
            "--release",
            "-p",
            "mimalloc-c",
            "--features",
            "secure",
            "--target-dir",
        ])
        .arg(&secure_dir)
        .status()?;
    if !st.success() {
        bail!("cargo build mimalloc-c --features secure failed");
    }
    let built = find_release_cdylib(&secure_dir)?;
    let dest = rust.join("target/release/libmimalloc-secure.so");
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::copy(&built, &dest)?;
    let link = rust.join("target/release/libmimalloc-secure.so.3");
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("libmimalloc-secure.so", &link).ok();
    }
    if which::which("readelf").is_ok() {
        let got = readelf_soname(&dest)?;
        if got != SECURE_SONAME {
            bail!("{} SONAME {got}, expected {SECURE_SONAME}", dest.display());
        }
    }
    Ok((default, dest))
}

/// glibc cdylib used as `LD_PRELOAD` / `ld-nix.so.preload` must DT_NEED
/// `libc.so.6` and must not leave an unversioned `U atexit` (that symbol is
/// only in `libc_nonshared.a`, so preload then fails with
/// `undefined symbol: atexit`).
pub fn glibc_cdylib_preload_ok(readelf_d: &str, nm_undefined: &str) -> Result<(), String> {
    if !readelf_d.contains("Shared library: [libc.so.6]") {
        return Err("cdylib is missing DT_NEEDED libc.so.6".into());
    }
    for line in nm_undefined.lines() {
        let line = line.trim();
        if line.contains(" U atexit") && !line.contains("@") && !line.contains("cxa_atexit") {
            return Err("unversioned U atexit (use __cxa_atexit; atexit is libc_nonshared)".into());
        }
        if line.ends_with(" U atexit") {
            return Err("unversioned U atexit (use __cxa_atexit; atexit is libc_nonshared)".into());
        }
    }
    Ok(())
}

pub fn check_glibc_cdylib_preload(so: &Path) -> Result<()> {
    let readelf = which::which("readelf").ok();
    let nm = which::which("nm").ok();
    let (Some(readelf), Some(nm)) = (readelf, nm) else {
        return Ok(());
    };
    let d = Command::new(readelf).arg("-d").arg(so).output()?;
    let u = Command::new(nm).arg("-D").arg(so).output()?;
    glibc_cdylib_preload_ok(
        &String::from_utf8_lossy(&d.stdout),
        &String::from_utf8_lossy(&u.stdout),
    )
    .map_err(|e| anyhow::anyhow!("{}: {e}", so.display()))
}

#[cfg(test)]
mod soname_tests {
    use super::*;

    #[test]
    fn parse_default_and_secure_soname() {
        assert_eq!(
            parse_dt_soname(" 0x000000000000000e (SONAME) Library soname: [libmimalloc.so.3]\n")
                .as_deref(),
            Some(DEFAULT_SONAME)
        );
        assert_eq!(
            parse_dt_soname(
                " 0x000000000000000e (SONAME) Library soname: [libmimalloc-secure.so.3]\n"
            )
            .as_deref(),
            Some(SECURE_SONAME)
        );
        assert!(parse_dt_soname("NEEDED libc.so.6\n").is_none());
    }

    #[test]
    fn expected_soname_from_name_or_target_dir() {
        assert_eq!(
            expected_soname(Path::new("target/release/libmimalloc.so")),
            DEFAULT_SONAME
        );
        assert_eq!(
            expected_soname(Path::new("target/release/libmimalloc-secure.so")),
            SECURE_SONAME
        );
        assert_eq!(
            expected_soname(Path::new("target/mimalloc-secure/release/libmimalloc.so")),
            SECURE_SONAME
        );
    }

    #[test]
    fn nix_cdylib_without_libc_or_raw_atexit_is_rejected() {
        let bad_d = " 0x000000000000000e (SONAME) Library soname: [libmimalloc.so.3]\n";
        let bad_nm = "                 U atexit\n                 U malloc\n";
        assert!(glibc_cdylib_preload_ok(bad_d, bad_nm).is_err());
        let good_d = " 0x0000000000000001 (NEEDED) Shared library: [libc.so.6]\n";
        let good_nm = "                 U __cxa_atexit@GLIBC_2.2.5\n";
        assert!(glibc_cdylib_preload_ok(good_d, good_nm).is_ok());
    }
}

pub fn write_all(path: &Path, data: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(data.as_bytes())?;
    Ok(())
}

pub fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn run_ok(
    program: impl AsRef<OsStr>,
    args: &[&str],
    extra_env: &[(&str, OsString)],
) -> Result<()> {
    let mut cmd = Command::new(program.as_ref());
    cmd.args(args);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let st = cmd.status()?;
    if !st.success() {
        bail!("{:?} failed with {}", program.as_ref(), status_code(st));
    }
    Ok(())
}
