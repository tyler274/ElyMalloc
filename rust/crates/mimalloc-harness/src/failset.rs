//! Oracle FAIL-set comparison: Rust regressions vs C mimalloc / jemalloc.

/// Names from `FAIL name` lines (full `all.txt` / `fail.txt` records).
pub fn fail_names(text: &str) -> Vec<String> {
    status_names(text, "FAIL")
}

pub fn status_names(text: &str, status: &str) -> Vec<String> {
    let prefix = format!("{status} ");
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix(&prefix).map(|n| n.to_string()))
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn rustc_fail_names(text: &str) -> Vec<String> {
    fail_names(text)
        .into_iter()
        .filter(|n| n.starts_with("rustc:"))
        .collect()
}

/// `comm -13 baseline rust` — names in `rust` that are not in `baseline`.
pub fn only_in_left(rust: &[String], baseline: &[String]) -> Vec<String> {
    let mut base = baseline.to_vec();
    base.sort();
    rust.iter()
        .filter(|n| base.binary_search(n).is_err())
        .cloned()
        .collect()
}

/// True if every Rust FAIL is also a baseline FAIL (C-only FAILs are ok).
pub fn rust_fail_subset_of(rust_fail_txt: &str, baseline_fail_txt: &str) -> bool {
    let mut rust = fail_names(rust_fail_txt);
    let mut base = fail_names(baseline_fail_txt);
    rust.sort();
    base.sort();
    only_in_left(&rust, &base).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_only_gxx_cxx_is_ok() {
        let rust = "FAIL rustc:foo.rs\n";
        let c = "FAIL gxx-cxx\nFAIL rustc:foo.rs\n";
        assert!(rust_fail_subset_of(rust, c));
        assert!(only_in_left(&fail_names(rust), &fail_names(c)).is_empty());
    }

    #[test]
    fn rust_only_fail_is_regression() {
        let rust = "FAIL rustc:hashset.rs\nFAIL gcc-smoke\n";
        let c = "FAIL gxx-cxx\n";
        let extra = only_in_left(&fail_names(rust), &fail_names(c));
        assert!(extra.iter().any(|n| n.contains("hashset")));
        assert!(!rust_fail_subset_of(rust, c));
    }

    #[test]
    fn rustc_names_only() {
        let t = "FAIL gxx-cxx\nFAIL rustc:cast__x.rs\nPASS rustc:ok.rs\n";
        assert_eq!(rustc_fail_names(t), vec!["rustc:cast__x.rs".to_string()]);
    }
}
