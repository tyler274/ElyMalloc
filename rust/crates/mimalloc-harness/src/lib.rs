//! Logic for the mimalloc rewrite test harness.
//!
//! Drivers live in this crate so filters, output comparison, FAIL-set diffs,
//! and WASM import checks can be unit-tested without shell.

pub mod bench;
pub mod cabi;
pub mod compare;
pub mod crash;
pub mod failset;
pub mod filter;
pub mod ids;
pub mod linkers;
pub mod normalize;
pub mod oracle;
pub mod preload;
pub mod process;
pub mod run;
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

pub fn env_is_one(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("1")
}

pub fn which(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}
