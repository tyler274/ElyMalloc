//! CLI for the rewrite test harness ([`elymalloc_harness`]).

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "elymalloc-harness",
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
    /// CPython regrtest (python/cpython Lib/test) vs C mimalloc and libc
    Python,
    /// Leptos (leptos-rs/leptos) WASM test suites + reactive_graph smoke
    Leptos,
    /// Property tests, heap fuzzer, and threaded chaos monkey (longer than cargo test)
    Fuzz {
        /// Heap-fuzzer / chaos step budget (`MIMALLOC_CHAOS_STEPS`)
        #[arg(long, default_value_t = elymalloc_harness::fuzz::DEFAULT_STEPS)]
        steps: u32,
        /// RNG seed (`MIMALLOC_CHAOS_SEED`)
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Vulkan Memory Allocator C ABI (virtual allocator, 3.4 symbols, Blender-style smoke)
    Vma,
    /// Firefox / Chromium / Electron vs C mimalloc (startup, child maps, page smoke)
    Browsers,
    /// LLVM PGO: instrument, train, merge, rebuild with profile-use
    Pgo,
    /// Run PGO training workloads (caller sets LLVM_PROFILE_FILE)
    PgoTrain(elymalloc_harness::pgo::TrainArgs),
}

fn main() {
    let cli = Cli::parse();
    let r: Result<()> = match cli.cmd {
        Cmd::Run => elymalloc_harness::run::run(),
        Cmd::CAbi => elymalloc_harness::cabi::run(),
        Cmd::WasmSmoke => elymalloc_harness::wasm_smoke::run(),
        Cmd::CompilerPreload => elymalloc_harness::preload::run(),
        Cmd::Oracle => elymalloc_harness::oracle::run(),
        Cmd::Linkers => elymalloc_harness::linkers::run(),
        Cmd::Bench => elymalloc_harness::bench::run(),
        Cmd::World => elymalloc_harness::world::run(),
        Cmd::Projects => elymalloc_harness::projects::run(),
        Cmd::Python => elymalloc_harness::python::run(),
        Cmd::Leptos => elymalloc_harness::leptos::run(),
        Cmd::Fuzz { steps, seed } => elymalloc_harness::fuzz::run(steps, seed),
        Cmd::Vma => {
            #[cfg(unix)]
            {
                elymalloc_harness::vma::run()
            }
            #[cfg(not(unix))]
            {
                Err(anyhow::anyhow!("vma suite is unix-only"))
            }
        }
        Cmd::Browsers => elymalloc_harness::browsers::run(),
        Cmd::Pgo => elymalloc_harness::pgo::pgo(),
        Cmd::PgoTrain(args) => elymalloc_harness::pgo::train(args),
    };
    if let Err(e) = r {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
