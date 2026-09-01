//! Logic for the mimalloc rewrite test harness.
//!
//! Drivers live here so filters, output comparison, FAIL-set diffs, and WASM
//! import checks can be unit-tested without shell. The binary
//! (`mimalloc-harness`) is a thin clap front-end.
//!
//! | Module | `mimalloc-harness` subcommand |
//! |--------|-------------------------------|
//! | [`run`] | `run` - cargo tests + C ABI + wasm smoke |
//! | [`cabi`] | `c-abi` |
//! | [`wasm_smoke`] | `wasm-smoke` |
//! | [`preload`] | `compiler-preload` |
//! | [`oracle`] | `oracle` (rewrite ⊆ C mimalloc and jemalloc) |
//! | [`linkers`] | `linkers` |
//! | [`mod@bench`] | `bench` |
//! | [`world`] | `world` |
//! | [`projects`] | `projects` (Bun, Serde) |
//! | [`leptos`] | `leptos` (Leptos WASM suites) |
//! | [`vma`] | `vma` (AMD VMA 3.4) |
//! | [`browsers`] | `browsers` |

pub mod bench;
pub mod browsers;
pub mod cabi;
pub mod compare;
pub mod crash;
pub mod failset;
pub mod filter;
pub mod ids;
pub mod leptos;
pub mod linkers;
pub mod normalize;
pub mod oracle;
pub mod preload;
pub mod process;
pub mod projects;
pub mod run;
pub mod vma;
pub mod wasm;
pub mod wasm_smoke;
pub mod world;

use std::path::{Path, PathBuf};

/// `rust/` directory (workspace containing this crate).
pub fn rust_root() -> PathBuf {
    if let Ok(p) = std::env::var("MIMALLOC_RUST_ROOT") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/mimalloc-harness is two levels under rust/")
        .to_path_buf()
}

/// Repository root (C mimalloc, `include/`, `test/`).
pub fn repo_root() -> PathBuf {
    rust_root()
        .parent()
        .expect("rust/ is inside the mimalloc repo")
        .to_path_buf()
}

/// `true` if environment variable `name` is the string `1`.
pub fn env_is_one(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("1")
}

/// `PATH` lookup; `None` if the program is missing.
pub fn which(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}
