//! WASM import section: fail if a libc-style `malloc` is imported.
//!
//! `wasm32-unknown-unknown` must not import `env::malloc`. WASI may import
//! `wasi_snapshot_preview1` but not libc malloc.

use std::path::Path;

use anyhow::{bail, Context};
use regex::Regex;
use wasmparser::{Parser, Payload};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmImportPolicy {
    /// wasm32-unknown-unknown: no imports at all.
    ExpectNone,
    /// wasm32-wasip1: WASI preview imports only.
    AllowWasi,
    /// No libc/emscripten malloc. wasm-bindgen / JS host imports are allowed
    /// (Leptos CSR / `web-sys`).
    NoLibc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmImport {
    pub module: String,
    pub name: String,
}

const LIBC_NAMES: &[&str] = &[
    "malloc",
    "calloc",
    "realloc",
    "free",
    "posix_memalign",
    "aligned_alloc",
    "mmap",
    "munmap",
    "sbrk",
    "__libc_malloc",
];

const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi", "wasi_snapshot_preview0"];

pub fn check_imports(imps: &[WasmImport], policy: WasmImportPolicy) -> Result<(), Vec<String>> {
    let mut forbidden = Vec::new();
    for imp in imps {
        let libcy = LIBC_NAMES.contains(&imp.name.as_str());
        let env_malloc =
            matches!(imp.module.as_str(), "env" | "emscripten") && imp.name.contains("malloc");
        if libcy || env_malloc {
            forbidden.push(format!("{}.{}", imp.module, imp.name));
        }
        match policy {
            WasmImportPolicy::ExpectNone => {
                forbidden.push(format!("{}.{}", imp.module, imp.name));
            }
            WasmImportPolicy::AllowWasi => {
                if !WASI_MODULES.contains(&imp.module.as_str()) {
                    forbidden.push(format!("{}.{}", imp.module, imp.name));
                }
            }
            WasmImportPolicy::NoLibc => {}
        }
    }
    forbidden.sort();
    forbidden.dedup();
    if forbidden.is_empty() {
        Ok(())
    } else {
        Err(forbidden)
    }
}

pub fn cargo_tree_has_libc(tree: &str) -> bool {
    Regex::new(r"(^|[[:space:]])libc[[:space:]]")
        .unwrap()
        .is_match(tree)
}

pub fn wasm_imports(data: &[u8]) -> anyhow::Result<Vec<WasmImport>> {
    if data.len() < 8 || &data[..4] != b"\0asm" {
        bail!("not a wasm module");
    }
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(data) {
        let payload = payload.context("wasm parse")?;
        if let Payload::ImportSection(reader) = payload {
            for imp in reader {
                let imp = imp.context("wasm import")?;
                out.push(WasmImport {
                    module: imp.module.to_string(),
                    name: imp.name.to_string(),
                });
            }
        }
    }
    Ok(out)
}

pub fn wasm_imports_file(path: &Path) -> anyhow::Result<Vec<WasmImport>> {
    let data = std::fs::read(path).with_context(|| format!("read {path:?}"))?;
    wasm_imports(&data)
}

pub fn ensure_wasm_targets() -> anyhow::Result<()> {
    let rustup = which::which("rustup").context("wasm: rustup not found")?;
    let st = std::process::Command::new(rustup)
        .args(["target", "add", "wasm32-unknown-unknown", "wasm32-wasip1"])
        .status()?;
    if !st.success() {
        anyhow::bail!("rustup target add failed");
    }
    Ok(())
}

pub fn find_wasmtime() -> Option<std::path::PathBuf> {
    if let Ok(p) = which::which("wasmtime") {
        return Some(p);
    }
    for cand in ["/run/current-system/sw/bin/wasmtime", "/usr/bin/wasmtime"] {
        let p = std::path::PathBuf::from(cand);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(nix) = which::which("nix-build") {
        let out = std::process::Command::new(nix)
            .args(["--no-out-link", "<nixpkgs>", "-A", "wasmtime"])
            .output()
            .ok()?;
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let bin = std::path::PathBuf::from(p).join("bin/wasmtime");
            if bin.is_file() {
                return Some(bin);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leb(mut n: u32) -> Vec<u8> {
        let mut o = Vec::new();
        loop {
            let mut b = (n & 0x7f) as u8;
            n >>= 7;
            if n != 0 {
                b |= 0x80;
            }
            o.push(b);
            if n == 0 {
                break;
            }
        }
        o
    }

    fn module_with_func_import(module: &str, name: &str) -> Vec<u8> {
        let mut type_payload = Vec::new();
        type_payload.extend(leb(1));
        type_payload.push(0x60);
        type_payload.extend(leb(0));
        type_payload.extend(leb(0));
        let mut type_sec = vec![1];
        type_sec.extend(leb(type_payload.len() as u32));
        type_sec.extend(type_payload);

        let mut imp_payload = Vec::new();
        imp_payload.extend(leb(1));
        imp_payload.extend(leb(module.len() as u32));
        imp_payload.extend(module.bytes());
        imp_payload.extend(leb(name.len() as u32));
        imp_payload.extend(name.bytes());
        imp_payload.push(0);
        imp_payload.extend(leb(0));
        let mut imp_sec = vec![2];
        imp_sec.extend(leb(imp_payload.len() as u32));
        imp_sec.extend(imp_payload);

        let mut m = b"\0asm\x01\x00\x00\x00".to_vec();
        m.extend(type_sec);
        m.extend(imp_sec);
        m
    }

    #[test]
    fn empty_module_ok() {
        let m = b"\0asm\x01\x00\x00\x00";
        assert!(wasm_imports(m).unwrap().is_empty());
        assert!(check_imports(&[], WasmImportPolicy::ExpectNone).is_ok());
    }

    #[test]
    fn unknown_unknown_rejects_any_import() {
        let m = module_with_func_import("env", "foo");
        let imps = wasm_imports(&m).unwrap();
        assert_eq!(imps[0].name, "foo");
        assert!(check_imports(&imps, WasmImportPolicy::ExpectNone).is_err());
    }

    #[test]
    fn wasi_ok_malloc_not() {
        let wasi = [WasmImport {
            module: "wasi_snapshot_preview1".into(),
            name: "fd_write".into(),
        }];
        assert!(check_imports(&wasi, WasmImportPolicy::AllowWasi).is_ok());
        let malloc = [WasmImport {
            module: "env".into(),
            name: "malloc".into(),
        }];
        assert!(check_imports(&malloc, WasmImportPolicy::AllowWasi).is_err());
        assert!(check_imports(&malloc, WasmImportPolicy::ExpectNone).is_err());
        assert!(check_imports(&malloc, WasmImportPolicy::NoLibc).is_err());
        let wbg = [WasmImport {
            module: "__wbindgen_placeholder__".into(),
            name: "__wbindgen_describe".into(),
        }];
        assert!(check_imports(&wbg, WasmImportPolicy::NoLibc).is_ok());
        assert!(check_imports(&wbg, WasmImportPolicy::ExpectNone).is_err());
    }

    #[test]
    fn cargo_tree_libc_line() {
        assert!(cargo_tree_has_libc("mimalloc-core v1\n libc v0.2.0\n"));
        assert!(!cargo_tree_has_libc("mimalloc-core v1\n"));
        assert!(!cargo_tree_has_libc("notlibc v1\n"));
    }
}
