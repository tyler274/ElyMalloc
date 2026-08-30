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
}

fn main() {
    let cli = Cli::parse();
    let r: Result<()> = match cli.cmd {
        Cmd::Run => mimalloc_harness::run::run(),
        Cmd::CAbi => mimalloc_harness::cabi::run(),
        Cmd::WasmSmoke => mimalloc_harness::wasm_smoke::run(),
        Cmd::CompilerPreload => mimalloc_harness::preload::run(),
        Cmd::Oracle => mimalloc_harness::oracle::run(),
    };
    if let Err(e) = r {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
