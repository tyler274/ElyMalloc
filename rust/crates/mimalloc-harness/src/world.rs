//! Typical NixOS-world CLI packages under `LD_PRELOAD` of the rewrite.

use std::time::Duration;

use anyhow::{bail, Result};

use crate::process::{build_mimalloc_cdylibs, compile, run_captured};
use crate::rust_root;

struct Probe {
    bin: &'static str,
    args: &'static [&'static str],
}

const PROBES: &[Probe] = &[
    Probe {
        bin: "git",
        args: &["--version"],
    },
    Probe {
        bin: "curl",
        args: &["--version"],
    },
    Probe {
        bin: "rg",
        args: &["--version"],
    },
    Probe {
        bin: "jq",
        args: &["-n", "1"],
    },
    Probe {
        bin: "python3",
        args: &["-c", "print(sum(range(1000)))"],
    },
    Probe {
        bin: "perl",
        args: &["-e", "print 1"],
    },
    Probe {
        bin: "openssl",
        args: &["version"],
    },
    Probe {
        bin: "gcc",
        args: &["--version"],
    },
    Probe {
        bin: "make",
        args: &["--version"],
    },
];

pub fn run() -> Result<()> {
    let (so, _) = build_mimalloc_cdylibs()?;
    println!("==> world packages under LD_PRELOAD {}", so.display());
    let timeout = Duration::from_secs(60);
    let extra = [("LD_PRELOAD", so.as_os_str().to_os_string())];
    let mut ran = 0usize;
    for p in PROBES {
        let Some(bin) = crate::which(p.bin) else {
            println!("skip {} (not on PATH)", p.bin);
            continue;
        };
        let cap = run_captured(&bin, p.args, &extra, timeout)?;
        if cap.rc != 0 {
            bail!(
                "{} {:?} exited {} stderr={}",
                p.bin,
                p.args,
                cap.rc,
                String::from_utf8_lossy(&cap.stderr)
            );
        }
        println!("ok {} {:?}", p.bin, p.args);
        ran += 1;
    }

    if let Some(cc) = crate::which("cc").or_else(|| crate::which("gcc")) {
        let out_dir = rust_root().join("target/world-preload");
        std::fs::create_dir_all(&out_dir)?;
        let src = out_dir.join("t.c");
        std::fs::write(
            &src,
            "#include <stdlib.h>\nint main(void){void*p=malloc(64);free(p);return 0;}\n",
        )?;
        let bin = out_dir.join("t");
        compile(&cc, &[src.to_str().unwrap(), "-O2"], &bin)?;
        let cap = run_captured(&bin, &[], &extra, timeout)?;
        if cap.rc != 0 {
            bail!("compiled malloc smoke exited {}", cap.rc);
        }
        println!("ok cc malloc/free");
        ran += 1;
    }

    if ran == 0 {
        bail!("no world probes found on PATH");
    }
    println!("world-preload: {ran} probes ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn probes_have_names() {
        assert!(!super::PROBES.is_empty());
    }
}
