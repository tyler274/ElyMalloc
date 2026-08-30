//! Which GCC torture / rustc UI files the harness should try to compile and run.

/// Skip DejaGNU glue and files that need extra sources / trap.
pub fn skip_c_torture_source(src: &str) -> bool {
    src.contains("__builtin_trap")
        || src.contains("dg-error")
        || src.contains("dg-require-effective-target")
}

/// rustc `tests/ui` run-pass cases we can compile with a plain `rustc --edition 2021`.
pub fn rustc_ui_include(src: &str) -> bool {
    if !src.contains("//@ run-pass") {
        return false;
    }
    if src.contains("//~") {
        return false;
    }
    for line in src.lines() {
        let Some(rest) = directive_rest(line) else {
            continue;
        };
        if rest.starts_with("aux-build")
            || rest.starts_with("edition")
            || rest.starts_with("feature")
            || rest.starts_with("ignore")
            || rest.starts_with("needs-")
            || rest.starts_with("revisions")
        {
            return false;
        }
    }
    true
}

fn directive_rest(line: &str) -> Option<&str> {
    let idx = line.find("//@")?;
    Some(line[idx + 3..].trim_start())
}

pub const RUST_UI_LIST_VER: &str = "exec-output-v1";

pub fn rustc_ui_list_current(text: &str) -> bool {
    text.lines().any(|l| l == format!("# {RUST_UI_LIST_VER}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_pass_plain_included() {
        assert!(rustc_ui_include("//@ run-pass\n\nfn main() {}\n"));
    }

    #[test]
    fn check_run_results_kept() {
        let src = "//@ run-pass\n//@ check-run-results\n\nfn main() { println!(\"hi\"); }\n";
        assert!(rustc_ui_include(src));
    }

    #[test]
    fn aux_build_skipped() {
        assert!(!rustc_ui_include(
            "//@ run-pass\n//@ aux-build: foo.rs\nfn main() {}\n"
        ));
    }

    #[test]
    fn compile_fail_patterns_skipped() {
        assert!(!rustc_ui_include(
            "//@ run-pass\nfn main() {}\n//~ ERROR foo\n"
        ));
    }

    #[test]
    fn not_run_pass() {
        assert!(!rustc_ui_include("//@ compile-fail\nfn main() {}\n"));
    }

    #[test]
    fn c_dg_error_skipped() {
        assert!(skip_c_torture_source(
            "/* { dg-error \"foo\" } */\nint main(){}\n"
        ));
        assert!(skip_c_torture_source("x = __builtin_trap();\n"));
        assert!(!skip_c_torture_source("int main(void) { return 0; }\n"));
    }

    #[test]
    fn list_version_line() {
        assert!(rustc_ui_list_current("# exec-output-v1\n/a.rs\n"));
        assert!(!rustc_ui_list_current("/a.rs\n"));
    }
}
