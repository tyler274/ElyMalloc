//! rustc/lld dying under LD_PRELOAD often returns 1 (FatalError), not 128+signal.

pub fn is_compiler_crash(rc: i32, stderr: &str) -> bool {
    if rc >= 128 {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "failed to initiate panic",
        "fatal runtime error",
        "LLVM ERROR",
        "signal: 6",
        "signal: 11",
        "SIGABRT",
        "SIGSEGV",
        "Aborted (core dumped)",
        "Segmentation fault",
    ];
    NEEDLES.iter().any(|n| stderr.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_exit() {
        assert!(is_compiler_crash(134, ""));
        assert!(is_compiler_crash(139, ""));
        assert!(!is_compiler_crash(1, "error: cannot find type `Foo`"));
    }

    #[test]
    fn fatal_error_text() {
        assert!(is_compiler_crash(
            1,
            "error: failed to initiate panic, error 3\n"
        ));
        assert!(!is_compiler_crash(
            1,
            "error: aborting due to previous error"
        ));
    }
}
