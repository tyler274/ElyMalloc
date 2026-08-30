use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::Path;
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
    let mut cmd = Command::new(program.as_ref());
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
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
