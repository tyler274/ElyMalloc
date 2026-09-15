//! Compare captured runs: exit code must match; stdout/stderr may be ASLR-normalized.

use crate::normalize::normalize_text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub rc: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    Normalized,
    Mismatch,
}

impl Captured {
    pub fn from_utf8_lossy_parts(stdout: &str, stderr: &str, rc: i32) -> Self {
        Self {
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            rc,
        }
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

pub fn compare_captured(sys: &Captured, pre: &Captured) -> MatchKind {
    if sys.rc != pre.rc {
        return MatchKind::Mismatch;
    }
    if sys.stdout == pre.stdout && sys.stderr == pre.stderr {
        return MatchKind::Exact;
    }
    if normalize_text(&sys.stdout_str()) == normalize_text(&pre.stdout_str())
        && normalize_text(&sys.stderr_str()) == normalize_text(&pre.stderr_str())
    {
        return MatchKind::Normalized;
    }
    MatchKind::Mismatch
}

pub fn outputs_match(sys: &Captured, pre: &Captured) -> bool {
    compare_captured(sys, pre) != MatchKind::Mismatch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_pass() {
        let a = Captured::from_utf8_lossy_parts("smoke ok\n", "", 0);
        assert_eq!(compare_captured(&a, &a), MatchKind::Exact);
    }

    #[test]
    fn aslr_stdout_is_ok_if_rc_matches() {
        let sys = Captured::from_utf8_lossy_parts("&x=7ffc0d0e1568\n", "", 0);
        let pre = Captured::from_utf8_lossy_parts("&x=7ffd07907688\n", "", 0);
        assert_eq!(compare_captured(&sys, &pre), MatchKind::Normalized);
        assert!(outputs_match(&sys, &pre));
    }

    #[test]
    fn crash_vs_success_is_fail() {
        let sys = Captured::from_utf8_lossy_parts("", "", 0);
        let pre = Captured::from_utf8_lossy_parts("", "timeout: dumped core\n", 139);
        assert_eq!(compare_captured(&sys, &pre), MatchKind::Mismatch);
    }

    #[test]
    fn missing_print_is_fail() {
        let sys = Captured::from_utf8_lossy_parts("1\n", "", 0);
        let pre = Captured::from_utf8_lossy_parts("", "", 0);
        assert_eq!(compare_captured(&sys, &pre), MatchKind::Mismatch);
    }

    #[test]
    fn thread_id_stderr_normalized() {
        let sys = Captured::from_utf8_lossy_parts(
            "",
            "thread 'main' (1) panicked at x.rs:1:1:\nexplicit panic\n",
            0,
        );
        let pre = Captured::from_utf8_lossy_parts(
            "",
            "thread 'main' (99) panicked at x.rs:1:1:\nexplicit panic\n",
            0,
        );
        assert_eq!(compare_captured(&sys, &pre), MatchKind::Normalized);
    }
}
