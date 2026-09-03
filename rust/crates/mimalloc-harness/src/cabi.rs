//! C ABI / LD_PRELOAD checks against `libmimalloc.so`.
//!
//! Compiles `rust/tests` and upstream `test/` smokes, then runs them with
//! `SO` preloaded. `INCLUDE` is the C `include/` tree. A second pass uses
//! the secure SONAME copy.

use std::ffi::OsString;
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

fn default_cdylib() -> PathBuf {
    let rust = rust_root();
    #[cfg(windows)]
    {
        rust.join("target/release/mimalloc.dll")
    }
    #[cfg(target_os = "macos")]
    {
        rust.join("target/release/libmimalloc.dylib")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        rust.join("target/release/libmimalloc.so")
    }
}

fn insert_env(so: &Path) -> (&'static str, OsString) {
    #[cfg(target_os = "macos")]
    {
        ("DYLD_INSERT_LIBRARIES", OsString::from(so.as_os_str()))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ("LD_PRELOAD", OsString::from(so.as_os_str()))
    }
    #[cfg(windows)]
    {
        (
            "PATH",
            OsString::from(so.parent().unwrap_or(so).as_os_str()),
        )
    }
}

fn lib_path_env(dir: &Path) -> (&'static str, OsString) {
    #[cfg(target_os = "macos")]
    {
        let mut lpath = dir.as_os_str().to_os_string();
        if let Some(prev) = std::env::var_os("DYLD_LIBRARY_PATH") {
            lpath.push(":");
            lpath.push(prev);
        }
        ("DYLD_LIBRARY_PATH", lpath)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut lpath = dir.as_os_str().to_os_string();
        if let Some(prev) = std::env::var_os("LD_LIBRARY_PATH") {
            lpath.push(":");
            lpath.push(prev);
        }
        ("LD_LIBRARY_PATH", lpath)
    }
    #[cfg(windows)]
    {
        let mut lpath = dir.as_os_str().to_os_string();
        if let Some(prev) = std::env::var_os("PATH") {
            lpath.push(";");
            lpath.push(prev);
        }
        ("PATH", lpath)
    }
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

#[cfg(all(unix, not(target_os = "linux")))]
fn versioned_lib_name(so: &Path) -> String {
    let secure = crate::process::expected_soname(so) == crate::process::SECURE_SONAME;
    if cfg!(target_os = "macos") {
        if secure {
            "libmimalloc-secure.3.dylib".into()
        } else {
            "libmimalloc.3.dylib".into()
        }
    } else {
        crate::process::expected_soname(so).to_string()
    }
}

fn soname_link(so: &Path) -> Result<PathBuf> {
    let dir = so.parent().context("so parent")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let versioned = {
            #[cfg(target_os = "linux")]
            {
                crate::process::readelf_soname(so)
                    .unwrap_or_else(|_| crate::process::expected_soname(so).to_string())
            }
            #[cfg(not(target_os = "linux"))]
            {
                versioned_lib_name(so)
            }
        };
        let link = dir.join(&versioned);
        if link != so {
            let _ = std::fs::remove_file(&link);
            symlink(so.file_name().context("so name")?, &link).ok();
        }
    }
    Ok(dir.to_path_buf())
}

/// C ABI / LD_PRELOAD checks. Env: `SO`, `INCLUDE`, `C_TESTS`, `UPSTREAM_TESTS`, optional `DEBUG_SO`, `OUT`.
pub fn run() -> Result<()> {
    let rust = rust_root();
    let repo = repo_root();
    let so =
        std::env::var("SO").unwrap_or_else(|_| default_cdylib().to_string_lossy().into_owned());
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

    #[cfg(windows)]
    {
        // MinGW `cc` (present on GHA windows-latest) cannot link `mimalloc.dll`
        // as an input object (`ld: cannot find \\mimalloc.dll`). LoadLibrary
        // exercises malloc/free/realloc/calloc without a C toolchain.
        let _ = (&include, &c_tests, &upstream, &out);
        windows_dll_smoke(&so)?;
        println!("c-abi windows smoke passed");
        return Ok(());
    }

    let sodir = soname_link(&so)?;
    #[cfg(target_os = "linux")]
    {
        if let Ok(readelf) = which::which("readelf") {
            let _ = Command::new(&readelf).arg("-d").arg(&so).status();
            if let Ok(name) = crate::process::readelf_soname(&so) {
                let want = crate::process::expected_soname(&so);
                if name != want {
                    bail!("{} SONAME {name}, expected {want}", so.display());
                }
            }
        }
        crate::process::check_glibc_cdylib_preload(&so)?;
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
    }

    let cc = cc();
    let inc = include.to_string_lossy().into_owned();
    let inc_arg = format!("-I{inc}");
    let so_s = so.to_string_lossy().into_owned();
    let preload = [insert_env(&so)];
    let libpath = [lib_path_env(&sodir)];

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
            let (k, dpath) = lib_path_env(&ddir);
            let fill_dbg = out.join("mi-api-fill-debug");
            compile(
                &cc,
                &["-O0", "-g", "-pthread", "-DMI_GUARDED=0", &inc_arg],
                &[&upstream.join("test-api-fill.c"), &debug],
                &fill_dbg,
            )?;
            run_ok(&fill_dbg, &[] as &[&str], &[(k, dpath)])?;
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
            &["-O2", "-pthread", "-std=c++17", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("cxx.cpp"), Path::new(&so_s)],
            &cxxb,
        )?;
        run_ok(&cxxb, &[] as &[&str], &libpath)?;
    }

    if !env_is_one("SKIP_RUST_ONLY") && !cfg!(windows) {
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
        let chaos = out.join("mi-chaos");
        compile(
            &cc,
            &["-O2", "-pthread", "-DNDEBUG", &inc_arg],
            &[&c_tests.join("chaos.c"), Path::new(&so_s)],
            &chaos,
        )?;
        run_ok(&chaos, &[] as &[&str], &libpath)?;
    }

    println!("c-abi checks passed");
    Ok(())
}

#[cfg(windows)]
fn windows_dll_smoke(dll: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    type Malloc = unsafe extern "C" fn(usize) -> *mut u8;
    type Free = unsafe extern "C" fn(*mut u8);
    type Realloc = unsafe extern "C" fn(*mut u8, usize) -> *mut u8;
    type Calloc = unsafe extern "C" fn(usize, usize) -> *mut u8;

    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
        fn GetProcAddress(h: *mut core::ffi::c_void, name: *const u8) -> *mut core::ffi::c_void;
        fn FreeLibrary(h: *mut core::ffi::c_void) -> i32;
    }

    unsafe fn proc<T>(h: *mut core::ffi::c_void, name: &[u8]) -> Result<T> {
        let p = GetProcAddress(h, name.as_ptr());
        if p.is_null() {
            bail!("missing export {}", String::from_utf8_lossy(name));
        }
        Ok(std::mem::transmute_copy(&p))
    }

    let mut w: Vec<u16> = dll.as_os_str().encode_wide().collect();
    w.push(0);
    let h = unsafe { LoadLibraryW(w.as_ptr()) };
    if h.is_null() {
        bail!("LoadLibrary {}", dll.display());
    }

    let result = (|| unsafe {
        let malloc: Malloc = proc(h, b"malloc\0")?;
        let free: Free = proc(h, b"free\0")?;
        let realloc: Realloc = proc(h, b"realloc\0")?;
        let calloc: Calloc = proc(h, b"calloc\0")?;
        let _mi: Malloc = proc(h, b"mi_malloc\0")?;

        let z = malloc(0);
        if z.is_null() {
            bail!("malloc(0)");
        }
        free(z);
        free(core::ptr::null_mut());

        let mut p = malloc(32);
        if p.is_null() {
            bail!("malloc");
        }
        core::ptr::write_bytes(p, 0xAB, 32);
        p = realloc(p, 4096);
        if p.is_null() {
            bail!("realloc grow");
        }
        if *p != 0xAB {
            bail!("realloc preserve");
        }
        let c = calloc(16, 4);
        if c.is_null() {
            bail!("calloc");
        }
        for i in 0..16 {
            if *c.add(i * 4) != 0 {
                bail!("calloc zero");
            }
        }
        free(p);
        free(c);
        Ok(())
    })();
    unsafe {
        FreeLibrary(h);
    }
    result
}
