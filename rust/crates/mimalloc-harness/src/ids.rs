use std::path::Path;

/// Unique rustc UI id: path under `tests/ui/` with `/` → `__`, no `.rs`.
/// Basename alone collides (`self.rs` exists in many directories).
pub fn rustc_test_id(path: &Path) -> String {
    let s = path.to_string_lossy();
    let rel = if let Some(idx) = s.rfind("/tests/ui/") {
        &s[idx + "/tests/ui/".len()..]
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(s.as_ref())
    };
    let rel = rel.strip_suffix(".rs").unwrap_or(rel);
    rel.replace('/', "__")
}

pub fn rustc_record_name(path: &Path) -> String {
    format!("rustc:{}", rustc_test_id(path))
}

/// Filesystem-safe capture key (no `/` or `:`).
pub fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | ':' | ' ' => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn same_basename_different_dirs_are_unique() {
        let a = rustc_test_id(Path::new("/x/rust-src/tests/ui/dyn/self.rs"));
        let b = rustc_test_id(Path::new("/x/rust-src/tests/ui/methods/self.rs"));
        assert_ne!(a, b);
        assert_eq!(a, "dyn__self");
        assert_eq!(b, "methods__self");
        assert_ne!(
            rustc_record_name(Path::new("/x/tests/ui/dyn/self.rs")),
            rustc_record_name(Path::new("/x/tests/ui/methods/self.rs"))
        );
    }

    #[test]
    fn nested_cast_id() {
        let p = Path::new(
            "/home/luluco/code/mimalloc/rust/target/compiler-stress/rust-src/tests/ui/cast/cast-region-to-uint.rs",
        );
        assert_eq!(rustc_test_id(p), "cast__cast-region-to-uint");
        assert_eq!(rustc_record_name(p), "rustc:cast__cast-region-to-uint");
    }
}
