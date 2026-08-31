use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mimalloc-harness",
    about = "Test harness for the Rust mimalloc rewrite"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// cargo tests, C ABI, and WASM smoke
    Run,
    /// C ABI / LD_PRELOAD checks (`SO`, `INCLUDE`, ...)
    CAbi,
    /// libc-less wasm32 smoke
    WasmSmoke,
    /// GCC/Clang/rustc suite: compile once, run under LD_PRELOAD, match output
    CompilerPreload,
    /// Rust vs C mimalloc vs jemalloc oracle
    Oracle,
    /// GNU ld, gold, LLD, mold, Wild + GlobalAlloc stress
    Linkers,
    /// Wall-clock and instruction-count malloc comparison
    Bench,
    /// NixOS-world package tests vs C mimalloc and libc
    World,
    /// Bun (oven-sh/bun) and Serde test suites vs C mimalloc and libc
    Projects,
    /// Vulkan Memory Allocator C ABI (virtual allocator, 3.4 symbols, Blender-style smoke)
    Vma,
    /// Firefox / Chromium / Electron vs C mimalloc (startup, child maps, page smoke)
    Browsers,
}

fn main() {
    let cli = Cli::parse();
    let r: Result<()> = match cli.cmd {
        Cmd::Run => mimalloc_harness::run::run(),
        Cmd::CAbi => mimalloc_harness::cabi::run(),
        Cmd::WasmSmoke => mimalloc_harness::wasm_smoke::run(),
        Cmd::CompilerPreload => mimalloc_harness::preload::run(),
        Cmd::Oracle => mimalloc_harness::oracle::run(),
        Cmd::Linkers => mimalloc_harness::linkers::run(),
        Cmd::Bench => mimalloc_harness::bench::run(),
        Cmd::World => mimalloc_harness::world::run(),
        Cmd::Projects => mimalloc_harness::projects::run(),
        Cmd::Vma => mimalloc_harness::vma::run(),
        Cmd::Browsers => mimalloc_harness::browsers::run(),
    };
    if let Err(e) = r {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
