use std::ffi::OsString;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::process::run_ok;
use crate::{env_is_one, repo_root, rust_root};

fn cc() -> PathBuf {
    std::env::var_os("CC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cc"))
}

fn cxx() -> PathBuf {
    std::env::var_os("CXX")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("c++"))
}

fn compile(cc: &Path, args: &[&str], srcs: &[&Path], out: &Path) -> Result<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut cmd = Command::new(cc);
    cmd.args(args);
    for s in srcs {
        cmd.arg(s);
    }
    cmd.arg("-o").arg(out);
    let st = cmd.status().context("cc")?;
    if !st.success() {
        bail!("compile failed: {}", out.display());
    }
    Ok(())
}

fn soname_link(so: &Path) -> Result<PathBuf> {
    let dir = so.parent().context("so parent")?;
    let link = dir.join("libmimalloc.so.3");
    let _ = std::fs::remove_file(&link);
    symlink(so.file_name().context("so name")?, &link).ok();
    Ok(dir.to_path_buf())
}

/// C ABI / LD_PRELOAD checks. Env: `SO`, `INCLUDE`, `C_TESTS`, `UPSTREAM_TESTS`, optional `DEBUG_SO`, `OUT`.
pub fn run() -> Result<()> {
    let rust = rust_root();
    let repo = repo_root();
    let so = std::env::var("SO").unwrap_or_else(|_| {
        rust.join("target/release/libmimalloc.so")
            .to_string_lossy()
            .into_owned()
    });
    let so = PathBuf::from(&so)
        .canonicalize()
        .with_context(|| format!("missing {so}"))?;
    let include = PathBuf::from(
        std::env::var("INCLUDE").unwrap_or_else(|_| repo.join("include").to_string_lossy().into()),
    );
    let c_tests = PathBuf::from(
        std::env::var("C_TESTS").unwrap_or_else(|_| rust.join("tests").to_string_lossy().into()),
    );
    let upstream = PathBuf::from(
        std::env::var("UPSTREAM_TESTS")
            .unwrap_or_else(|_| repo.join("test").to_string_lossy().into()),
    );
    let out = PathBuf::from(
        std::env::var("OUT").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into()),
    );
    std::fs::create_dir_all(&out)?;

    let sodir = soname_link(&so)?;
    if let Ok(readelf) = which::which("readelf") {
        let _ = Command::new(readelf).arg("-d").arg(&so).status();
    }
    if let Ok(nm) = which::which("nm") {
        let outp = Command::new(nm)
            .args(["-D", "--defined-only"])
            .arg(&so)
            .output()?;
        let text = String::from_utf8_lossy(&outp.stdout);
        if !text.lines().any(|l| {
            l.ends_with(" malloc")
                || l.ends_with(" free")
                || l.ends_with(" calloc")
                || l.ends_with(" realloc")
                || l.ends_with(" posix_memalign")
                || l.ends_with(" mi_malloc")
        }) {
            bail!("expected malloc/free/mi_malloc in {so:?}");
        }
    }

    let cc = cc();
    let inc = include.to_string_lossy().into_owned();
    let inc_arg = format!("-I{inc}");
    let so_s = so.to_string_lossy().into_owned();
    let preload = [("LD_PRELOAD", OsString::from(so.as_os_str()))];
    let mut lpath = sodir.as_os_str().to_os_string();
    if let Some(prev) = std::env::var_os("LD_LIBRARY_PATH") {
        lpath.push(":");
        lpath.push(prev);
    }
    let libpath = [("LD_LIBRARY_PATH", lpath)];

    let smoke = out.join("mi-smoke");
    compile(
        &cc,
        &["-O2", "-pthread"],
        &[&c_tests.join("smoke.c")],
        &smoke,
    )?;
    run_ok(&smoke, &[] as &[&str], &preload)?;

    let stress = out.join("mi-stress");
    compile(
        &cc,
        &["-O2", "-pthread", "-DUSE_STD_MALLOC", "-DNDEBUG", &inc_arg],
        &[&upstream.join("test-stress.c")],
        &stress,
    )?;
    run_ok(&stress, &["4", "10", "3"], &preload)?;

    let api = out.join("mi-api");
    compile(
        &cc,
        &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
        &[&upstream.join("test-api.c"), Path::new(&so_s)],
        &api,
    )?;
    run_ok(&api, &[] as &[&str], &libpath)?;

    if !env_is_one("SKIP_RUST_ONLY") {
        let theap = out.join("mi-theap");
        compile(
            &cc,
            &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("theap.c"), Path::new(&so_s)],
            &theap,
        )?;
        run_ok(&theap, &[] as &[&str], &libpath)?;
    }

    let fill = out.join("mi-api-fill");
    compile(
        &cc,
        &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
        &[&upstream.join("test-api-fill.c"), Path::new(&so_s)],
        &fill,
    )?;
    run_ok(&fill, &[] as &[&str], &libpath)?;

    if let Ok(debug) = std::env::var("DEBUG_SO") {
        let debug = PathBuf::from(debug);
        if debug.is_file() {
            let debug = debug.canonicalize()?;
            let ddir = soname_link(&debug)?;
            let mut dpath = ddir.as_os_str().to_os_string();
            if let Some(prev) = std::env::var_os("LD_LIBRARY_PATH") {
                dpath.push(":");
                dpath.push(prev);
            }
            let fill_dbg = out.join("mi-api-fill-debug");
            compile(
                &cc,
                &["-O0", "-g", "-pthread", "-DMI_GUARDED=0", &inc_arg],
                &[&upstream.join("test-api-fill.c"), &debug],
                &fill_dbg,
            )?;
            run_ok(&fill_dbg, &[] as &[&str], &[("LD_LIBRARY_PATH", dpath)])?;
        }
    }

    let heaps = out.join("mi-stress-heaps");
    compile(
        &cc,
        &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
        &[&upstream.join("test-stress-heaps.c"), Path::new(&so_s)],
        &heaps,
    )?;
    run_ok(&heaps, &["4", "10", "3"], &libpath)?;

    let sub = out.join("mi-stress-subprocs");
    compile(
        &cc,
        &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
        &[&upstream.join("test-stress-subprocs.c"), Path::new(&so_s)],
        &sub,
    )?;
    run_ok(&sub, &["4", "10", "3"], &libpath)?;

    if !env_is_one("SKIP_CXX") {
        let cxxb = out.join("mi-cxx");
        compile(
            &cxx(),
            &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("cxx.cpp"), Path::new(&so_s)],
            &cxxb,
        )?;
        run_ok(&cxxb, &[] as &[&str], &libpath)?;
    }

    if !env_is_one("SKIP_RUST_ONLY") {
        let proc = out.join("mi-process");
        compile(
            &cc,
            &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("process.c"), Path::new(&so_s)],
            &proc,
        )?;
        run_ok(&proc, &[] as &[&str], &libpath)?;
        let sec = out.join("mi-secure");
        compile(
            &cc,
            &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("secure.c"), Path::new(&so_s)],
            &sec,
        )?;
        run_ok(&sec, &[] as &[&str], &libpath)?;
    }

    println!("c-abi checks passed");
    Ok(())
}
