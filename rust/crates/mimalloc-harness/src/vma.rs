//! Vulkan Memory Allocator C ABI checks (virtual allocator + exported symbols).

use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::process::{cargo_ok, compile, run_ok};
use crate::rust_root;

const REQUIRED_SYMS: &[&str] = &[
    "vmaCreateAllocator",
    "vmaDestroyAllocator",
    "vmaCreateBuffer",
    "vmaDestroyBuffer",
    "vmaCreateImage",
    "vmaDestroyImage",
    "vmaAllocateMemory",
    "vmaFreeMemory",
    "vmaMapMemory",
    "vmaUnmapMemory",
    "vmaCreateVirtualBlock",
    "vmaDestroyVirtualBlock",
    "vmaVirtualAllocate",
    "vmaVirtualFree",
    "vmaCreatePool",
    "vmaDestroyPool",
    "vmaBeginDefragmentation",
    "vmaEndDefragmentation",
    "vmaBuildStatsString",
    "vmaFreeStatsString",
];

fn so_path() -> PathBuf {
    if let Ok(p) = std::env::var("VMA_SO") {
        return PathBuf::from(p);
    }
    let rust = rust_root();
    let release = rust.join("target/release/libVulkanMemoryAllocator.so");
    if release.is_file() {
        return release;
    }
    rust.join("target/debug/libVulkanMemoryAllocator.so")
}

pub fn run() -> Result<()> {
    if std::env::var_os("VMA_SO").is_none() {
        cargo_ok(&["test", "-p", "vma-core", "--release"])?;
        cargo_ok(&["build", "-p", "vma-c", "--release"])?;
    }

    let rust = rust_root();
    let so = so_path()
        .canonicalize()
        .with_context(|| "libVulkanMemoryAllocator.so")?;
    println!("==> VMA so {}", so.display());

    let nm = Command::new("nm")
        .args(["-D", "--defined-only"])
        .arg(&so)
        .output()
        .context("nm")?;
    if !nm.status.success() {
        bail!("nm failed on {}", so.display());
    }
    let names = String::from_utf8_lossy(&nm.stdout);
    for s in REQUIRED_SYMS {
        if !names.lines().any(|l| l.contains(s)) {
            bail!("missing exported symbol {s}");
        }
    }
    println!("ok {} vma* symbols", REQUIRED_SYMS.len());

    if let Ok(readelf) = which::which("readelf") {
        let out = Command::new(&readelf).args(["-d"]).arg(&so).output()?;
        let d = String::from_utf8_lossy(&out.stdout);
        if !d.contains("libVulkanMemoryAllocator.so.3") {
            bail!("SONAME is not libVulkanMemoryAllocator.so.3");
        }
        println!("ok SONAME libVulkanMemoryAllocator.so.3");
    }

    let include = rust.join("crates/vma-c/include");
    let src = rust.join("tests/vma-virtual.c");
    let out_dir = rust.join("target/vma-abi");
    let bin = out_dir.join("vma-virtual");
    let sodir = so.parent().unwrap();
    let soname = sodir.join("libVulkanMemoryAllocator.so.3");
    let _ = std::fs::remove_file(&soname);
    symlink(so.file_name().context("so name")?, &soname).ok();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let i_flag = format!("-I{}", include.display());
    let l_flag = format!("-L{}", sodir.display());
    let rpath = format!("-Wl,-rpath,{}", sodir.display());
    let src_s = src.to_string_lossy();
    compile(
        &cc,
        &[
            "-O1",
            "-Wall",
            "-Werror",
            &i_flag,
            src_s.as_ref(),
            &l_flag,
            "-lVulkanMemoryAllocator",
            &rpath,
        ],
        &bin,
    )?;
    run_ok(&bin, &[], &[])?;
    println!("ok vma-virtual");
    println!("all vma ABI checks passed");
    Ok(())
}
