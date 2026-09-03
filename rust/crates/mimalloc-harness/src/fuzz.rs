//! Longer seeded fuzz / chaos run (`mimalloc-harness fuzz`).
//!
//! Default `cargo test -p mimalloc-core` already runs the property tests, heap
//! fuzzer, and chaos monkey at a CI-sized step count. This subcommand raises
//! `MIMALLOC_CHAOS_STEPS` and re-runs those tests, then the C ABI `chaos.c`
//! probe (same binary as `c-abi`).

use anyhow::{Context, Result};

use crate::process::cargo_ok;

/// Default step budget when the user does not pass `--steps`.
pub const DEFAULT_STEPS: u32 = 65_536;

pub fn run(steps: u32, seed: u64) -> Result<()> {
    let steps = steps.max(1);
    std::env::set_var("MIMALLOC_CHAOS_STEPS", steps.to_string());
    std::env::set_var("MIMALLOC_CHAOS_SEED", seed.to_string());
    cargo_ok(&[
        "test",
        "-p",
        "mimalloc-core",
        "--release",
        "chaos::",
        "--",
        "--nocapture",
    ])?;
    crate::process::build_mimalloc_cdylibs()?;
    crate::cabi::run()?;
    Ok(())
}

pub fn parse_steps(raw: Option<&str>) -> Result<u32> {
    match raw {
        None => Ok(DEFAULT_STEPS),
        Some(s) => s.parse().with_context(|| format!("steps {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_steps_is_ci_larger() {
        assert!(DEFAULT_STEPS >= 8_192);
        assert_eq!(parse_steps(None).unwrap(), DEFAULT_STEPS);
        assert_eq!(parse_steps(Some("100")).unwrap(), 100);
        assert!(parse_steps(Some("nope")).is_err());
    }
}
