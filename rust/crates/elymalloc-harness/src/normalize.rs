//! Strip ASLR addresses and rustc thread ids so allocator output can be compared.

use std::sync::OnceLock;

use regex::Regex;

pub fn normalize_text(s: &str) -> String {
    let s = re_0x().replace_all(s, "<ADDR>");
    let s = re_tid().replace_all(&s, "thread <TID>");
    re_hexword().replace_all(&s, "<ADDR>").into_owned()
}

fn re_0x() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"0[xX][0-9a-fA-F]+").unwrap())
}

fn re_tid() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"thread '[^']+' \([0-9]+\)").unwrap())
}

fn re_hexword() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[0-9a-fA-F]{8,16}\b").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_addr_without_0x() {
        let a = normalize_text("&x=7ffc0d0e1568\n");
        let b = normalize_text("&x=7ffd07907688\n");
        assert_eq!(a, b);
        assert_eq!(a, "&x=<ADDR>\n");
    }

    #[test]
    fn hex_with_0x_prefix() {
        assert_eq!(normalize_text("p=0x7fffabc\n"), "p=<ADDR>\n");
    }

    #[test]
    fn rustc_thread_id() {
        let a = "thread 'main' (2861880) panicked at foo.rs:26:1:\n";
        let b = "thread 'main' (2861886) panicked at foo.rs:26:1:\n";
        assert_eq!(normalize_text(a), normalize_text(b));
        assert!(normalize_text(a).starts_with("thread <TID> panicked"));
    }

    #[test]
    fn smoke_ok_unchanged() {
        assert_eq!(normalize_text("smoke ok\n"), "smoke ok\n");
    }

    #[test]
    fn short_numbers_kept() {
        assert_eq!(normalize_text("vec size 1\n"), "vec size 1\n");
    }
}
