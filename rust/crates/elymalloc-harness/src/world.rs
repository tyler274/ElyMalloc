//! NixOS-world packages: run real workloads under the rewrite, C mimalloc, and libc.
//!
//! `--version` is not enough. Each probe must exercise malloc (repo roundtrips,
//! compression, compile/link, crypto, filesystem tools, …) and match libc
//! stdout/stderr/exit. Rewrite-only mismatches are FAIL; C-only mismatches are
//! noted. Injection matches NixOS `memoryAllocator`: bubblewrap over
//! `/etc/ld-nix.so.preload`. Binaries prefer `/run/current-system` and the
//! home-manager profile so Cursor-bundled tools (ripgrep) are not the corpus.
//! Optional `NIXOS_CONFIG` (default `/etc/nixos`) is scanned so the log shows
//! which probes correspond to this machine's config.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::browsers::{find_bwrap, preload_bind_dests};
use crate::compare::{outputs_match, Captured};
use crate::process::{build_mimalloc_cdylibs, run_captured_os};
use crate::rust_root;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Body {
    Args(&'static [&'static str]),
    Script(&'static str),
}

struct Probe {
    /// Stable oracle name (`git:init-commit`).
    name: &'static str,
    /// PATH / profile binary.
    bin: &'static str,
    /// Tokens looked up in `/etc/nixos` (nixpkgs attrs or extraPrograms).
    pkgs: &'static [&'static str],
    timeout_secs: u64,
    body: Body,
}

const PROBES: &[Probe] = &[
    Probe {
        name: "git:init-commit",
        bin: "git",
        pkgs: &["git"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
git init -q
git config user.name mi
git config user.email mi@example.invalid
printf 'hello mimalloc\n' > README
git add README
git commit -q -m m
git gc -q --prune=now
git log -1 --format=%s | grep -qx m
echo git-ok
"#,
        ),
    },
    Probe {
        name: "openssl:aes-roundtrip",
        bin: "openssl",
        pkgs: &["openssl"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
head -c 65536 /dev/zero > in.bin
openssl enc -aes-256-cbc -pbkdf2 -salt -pass pass:mimalloc -in in.bin -out enc.bin 2>/dev/null
openssl enc -d -aes-256-cbc -pbkdf2 -pass pass:mimalloc -in enc.bin -out out.bin 2>/dev/null
cmp in.bin out.bin
echo openssl-ok
"#,
        ),
    },
    Probe {
        name: "perl:alloc",
        bin: "perl",
        pkgs: &["perl"],
        timeout_secs: 20,
        body: Body::Args(&[
            "-e",
            r#"my $x = "a" x 200_000; my @h = map { $_ x 64 } 1..4000; die unless length($x)==200000; print "perl-ok\n""#,
        ]),
    },
    Probe {
        name: "python3:stdlib-slice",
        bin: "python3",
        pkgs: &["python3", "python3Full"],
        timeout_secs: 90,
        body: Body::Args(&[
            "-c",
            r#"import hashlib, json, threading
blob = bytes(i % 256 for i in range(50_000))
h = [hashlib.sha256(blob * 8).hexdigest() for _ in range(32)]
assert len(set(h)) == 1
out = []
def work(n):
    s = 0
    for i in range(n):
        s += i
    out.append(s)
ts = [threading.Thread(target=work, args=(20_000,)) for _ in range(8)]
for t in ts: t.start()
for t in ts: t.join()
assert out == [199990000] * 8
assert json.loads(json.dumps({"n": sum(range(1000))}))["n"] == 499500
print("python-ok")
"#,
        ]),
    },
    Probe {
        name: "nodejs:buffer-alloc",
        bin: "node",
        pkgs: &["nodejs", "nodejs_22", "nodejs_24", "node"],
        timeout_secs: 30,
        body: Body::Args(&[
            "-e",
            r#"const a=[]; for (let i=0;i<20000;i++) a.push(Buffer.alloc(128, i%256)); const {createHash}=require("crypto"); const h=createHash("sha256"); for (const b of a) h.update(b); if (a.length!==20000) process.exit(1); console.log("node-ok", a.length);"#,
        ]),
    },
    Probe {
        name: "gcc:malloc",
        bin: "gcc",
        pkgs: &["gcc"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
cat > t.c <<'EOF'
#include <stdlib.h>
#include <string.h>
int main(void) {
  char *p = malloc(4096);
  if (!p) return 1;
  memset(p, 0x5a, 4096);
  p = realloc(p, 65536);
  if (!p) return 2;
  memset(p + 4096, 0xa5, 65536 - 4096);
  free(p);
  return 0;
}
EOF
gcc -O2 t.c -o t
./t
echo gcc-ok
"#,
        ),
    },
    Probe {
        name: "gxx:new-delete",
        bin: "g++",
        pkgs: &["gcc", "gxx"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
cat > t.cpp <<'EOF'
#include <vector>
#include <string>
int main() {
  auto *p = new int[128];
  p[0] = 1;
  delete[] p;
  std::vector<std::string> v;
  for (int i = 0; i < 1000; i++) v.push_back(std::string(64, 'x'));
  return v.size() == 1000 ? 0 : 1;
}
EOF
g++ -O2 t.cpp -o tpp
./tpp
echo gxx-ok
"#,
        ),
    },
    Probe {
        name: "make:hello",
        bin: "make",
        pkgs: &["gnumake"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'all:\n\t@echo make-ok\n' > Makefile
make -s
"#,
        ),
    },
    Probe {
        name: "cmake:script",
        bin: "cmake",
        pkgs: &["cmake"],
        timeout_secs: 20,
        body: Body::Script(
            r#"set -euo pipefail
printf 'message("cmake-ok")\n' > t.cmake
cmake -P t.cmake
"#,
        ),
    },
    Probe {
        name: "zstd:roundtrip",
        bin: "zstd",
        pkgs: &["zstd"],
        timeout_secs: 20,
        body: Body::Script(
            r#"set -euo pipefail
head -c 262144 /dev/urandom > in.bin
zstd -q -3 in.bin -o in.bin.zst
zstd -q -d in.bin.zst -o out.bin
cmp in.bin out.bin
echo zstd-ok
"#,
        ),
    },
    Probe {
        name: "xz:roundtrip",
        bin: "xz",
        pkgs: &["xz"],
        timeout_secs: 20,
        body: Body::Script(
            r#"set -euo pipefail
head -c 131072 /dev/zero > in.bin
xz -c in.bin > in.bin.xz
xz -d -c in.bin.xz > out.bin
cmp in.bin out.bin
echo xz-ok
"#,
        ),
    },
    Probe {
        name: "gzip:roundtrip",
        bin: "gzip",
        pkgs: &["gzip"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'gzip-payload-%s\n' $(seq 1 2000) > in.txt
gzip -c in.txt > in.txt.gz
gzip -d -c in.txt.gz > out.txt
cmp in.txt out.txt
echo gzip-ok
"#,
        ),
    },
    Probe {
        name: "bzip2:roundtrip",
        bin: "bzip2",
        pkgs: &["bzip2"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'bzip-%s\n' $(seq 1 2000) > in.txt
bzip2 -c in.txt > in.txt.bz2
bzip2 -d -c in.txt.bz2 > out.txt
cmp in.txt out.txt
echo bzip2-ok
"#,
        ),
    },
    Probe {
        name: "lzop:roundtrip",
        bin: "lzop",
        pkgs: &["lzop"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
head -c 65536 /dev/zero > in.bin
lzop -c in.bin > in.bin.lzo
lzop -d -c in.bin.lzo > out.bin
cmp in.bin out.bin
echo lzop-ok
"#,
        ),
    },
    Probe {
        name: "p7zip:roundtrip",
        bin: "7z",
        pkgs: &["p7zip"],
        timeout_secs: 20,
        body: Body::Script(
            r#"set -euo pipefail
printf 'seven\n' > a.txt
7z a -bd t.7z a.txt >/dev/null
7z x -bd -y -oout t.7z >/dev/null
cmp a.txt out/a.txt
echo p7zip-ok
"#,
        ),
    },
    Probe {
        name: "tar:roundtrip",
        bin: "tar",
        pkgs: &["gnutar", "tar"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
mkdir d
printf 't\n' > d/a
tar -cf t.tar d
rm -rf d
tar -xf t.tar
printf 't\n' > expect
cmp d/a expect
echo tar-ok
"#,
        ),
    },
    Probe {
        name: "rg:search",
        bin: "rg",
        pkgs: &["ripgrep"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'alpha\nmimalloc-hit\nomega\n' > hay.txt
rg -x mimalloc-hit hay.txt >/dev/null
echo rg-ok
"#,
        ),
    },
    Probe {
        name: "jq:transform",
        bin: "jq",
        pkgs: &["jq"],
        timeout_secs: 10,
        body: Body::Args(&["-n", "{ok:true,n:([range(100)|.+1]|add)}"]),
    },
    Probe {
        name: "curl:file",
        bin: "curl",
        pkgs: &["curl"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'curl-ok\n' > page.txt
curl -fsS "file://$PWD/page.txt"
"#,
        ),
    },
    Probe {
        name: "wget:file",
        bin: "wget",
        pkgs: &["wget"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'wget-ok\n' > page.txt
wget -q -O - "file://$PWD/page.txt"
"#,
        ),
    },
    Probe {
        name: "vim:ex-silent",
        bin: "vim",
        pkgs: &["vim"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf 'vim-ok\n' > t.txt
vim -n -u NONE -i NONE -es -c 'wq' t.txt
grep -qx vim-ok t.txt
echo vim-ok
"#,
        ),
    },
    Probe {
        name: "nixfmt:format",
        bin: "nixfmt",
        pkgs: &["nixfmt"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
printf '{ a=1; b = 2; }\n' > t.nix
nixfmt t.nix
grep -q a t.nix
echo nixfmt-ok
"#,
        ),
    },
    Probe {
        name: "rustc:hello",
        bin: "rustc",
        pkgs: &["rustc", "rustup"],
        timeout_secs: 60,
        body: Body::Script(
            r#"set -euo pipefail
printf 'fn main(){println!("rustc-ok");}\n' > t.rs
rustc --edition 2021 t.rs -o t
./t
"#,
        ),
    },
    Probe {
        name: "mold:link",
        bin: "mold",
        pkgs: &["mold"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
printf 'int main(void){return 0;}\n' > t.c
cc -fuse-ld=mold t.c -o t
./t
echo mold-ok
"#,
        ),
    },
    Probe {
        name: "nix-instantiate:eval",
        bin: "nix-instantiate",
        pkgs: &["nix"],
        timeout_secs: 20,
        body: Body::Args(&["--eval", "-E", "1 + 1"]),
    },
    Probe {
        name: "e2fsprogs:mkfs-fsck",
        bin: "mkfs.ext4",
        pkgs: &["e2fsprogs"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
dd if=/dev/zero of=disk.img bs=1M count=8 status=none
mkfs.ext4 -F -q disk.img
e2fsck -n disk.img >/dev/null
echo e2fs-ok
"#,
        ),
    },
    Probe {
        name: "exfatprogs:mkfs",
        bin: "mkfs.exfat",
        pkgs: &["exfatprogs", "exfat"],
        timeout_secs: 20,
        body: Body::Script(
            r#"set -euo pipefail
dd if=/dev/zero of=disk.img bs=1M count=4 status=none
mkfs.exfat disk.img >/dev/null
echo exfat-ok
"#,
        ),
    },
    Probe {
        name: "ntfs3g:mkfs",
        bin: "mkfs.ntfs",
        pkgs: &["ntfs3g"],
        timeout_secs: 30,
        body: Body::Script(
            r#"set -euo pipefail
dd if=/dev/zero of=disk.img bs=1M count=8 status=none
mkfs.ntfs -F -q disk.img
echo ntfs-ok
"#,
        ),
    },
    Probe {
        name: "kubectl:client",
        bin: "kubectl",
        pkgs: &["kubectl"],
        timeout_secs: 15,
        body: Body::Args(&["version", "--client", "--output=yaml"]),
    },
    Probe {
        name: "pv:copy",
        bin: "pv",
        pkgs: &["pv"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
head -c 65536 /dev/zero > in.bin
pv -q -s 65536 in.bin > out.bin
cmp in.bin out.bin
echo pv-ok
"#,
        ),
    },
    Probe {
        name: "mbuffer:copy",
        bin: "mbuffer",
        pkgs: &["mbuffer"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
head -c 65536 /dev/zero > in.bin
mbuffer -q -s 64k -i in.bin -o out.bin
cmp in.bin out.bin
echo mbuffer-ok
"#,
        ),
    },
    Probe {
        name: "direnv:version",
        bin: "direnv",
        pkgs: &["direnv", "nix-direnv"],
        timeout_secs: 10,
        body: Body::Args(&["version"]),
    },
    Probe {
        name: "sbctl:help",
        bin: "sbctl",
        pkgs: &["sbctl"],
        timeout_secs: 10,
        body: Body::Args(&["--help"]),
    },
    Probe {
        name: "htop:version",
        bin: "htop",
        pkgs: &["htop"],
        timeout_secs: 10,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "gh:version",
        bin: "gh",
        pkgs: &["gh"],
        timeout_secs: 10,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "nil:version",
        bin: "nil",
        pkgs: &["nil"],
        timeout_secs: 10,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "nixd:version",
        bin: "nixd",
        pkgs: &["nixd"],
        timeout_secs: 10,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "hydra-check:help",
        bin: "hydra-check",
        pkgs: &["hydra-check"],
        timeout_secs: 10,
        body: Body::Args(&["--help"]),
    },
    Probe {
        name: "yt-dlp:version",
        bin: "yt-dlp",
        pkgs: &["yt-dlp"],
        timeout_secs: 15,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "kwin:virtual-session",
        bin: "kwin_wayland",
        pkgs: &["kwin", "kdePackages", "plasma6", "plasma"],
        timeout_secs: 45,
        body: Body::Script(
            r#"set -euo pipefail
export XDG_RUNTIME_DIR="$PWD/run"
export XDG_CONFIG_HOME="$PWD/config"
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME"
printf '%s\n' '#!/bin/sh' 'echo kwin-ok' > sess.sh
chmod +x sess.sh
# Nested virtual framebuffer; do not touch the live session.
kwin_wayland --virtual --width 800 --height 600 \
  --socket "mi-world-$$" --no-lockscreen --lock \
  --exit-with-session "$PWD/sess.sh" >/dev/null 2>&1
echo kwin-ok
"#,
        ),
    },
    Probe {
        name: "plasmashell:version",
        bin: "plasmashell",
        pkgs: &["plasma-workspace", "kdePackages", "plasma6"],
        timeout_secs: 15,
        body: Body::Args(&["--version"]),
    },
    Probe {
        name: "kreadconfig6:roundtrip",
        bin: "kwriteconfig6",
        pkgs: &["kconfig", "kdePackages", "plasma6"],
        timeout_secs: 15,
        body: Body::Script(
            r#"set -euo pipefail
kwriteconfig6 --file "$PWD/t.conf" --group g --key k mimalloc-ok
got=$(kreadconfig6 --file "$PWD/t.conf" --group g --key k)
test "$got" = mimalloc-ok
echo kconfig-ok
"#,
        ),
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Skip,
    FailRewrite,
    FailBoth,
}

fn judge(libc: &Captured, rust: &Captured, c: Option<&Captured>) -> Verdict {
    if libc.rc != 0 {
        return Verdict::Skip;
    }
    if outputs_match(libc, rust) {
        return Verdict::Pass;
    }
    match c {
        Some(c) if !outputs_match(libc, c) => Verdict::FailBoth,
        _ => Verdict::FailRewrite,
    }
}

fn is_cursor_tree(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains("/cursor/") || s.contains("/.cursor/") || s.contains("-cursor-")
}

fn is_hidden_bin(p: &Path) -> bool {
    is_cursor_tree(p) || p.to_string_lossy().contains("/run/wrappers/")
}

fn nix_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/run/current-system/sw/bin")];
    if let Ok(user) = std::env::var("USER") {
        dirs.push(PathBuf::from(format!("/etc/profiles/per-user/{user}/bin")));
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(format!("{home}/.nix-profile/bin")));
    }
    dirs
}

fn store_pkg_ok(store_name: &str, bin: &str) -> bool {
    match bin {
        "python3" => {
            store_name.contains("-python3-3.")
                && !store_name.contains("-env")
                && !store_name.contains("debug")
        }
        "node" => {
            store_name.contains("-nodejs-")
                && !store_name.contains("slim")
                && !store_name.contains("debug")
                && !store_name.contains("-dev")
        }
        _ => false,
    }
}

/// Numeric version after `-python3-` / `-nodejs-`, so 3.14 beats 3.13
/// regardless of the Nix store hash prefix.
fn store_version_nums(store_name: &str, bin: &str) -> Vec<u32> {
    let marker = match bin {
        "python3" => "-python3-",
        "node" => "-nodejs-",
        _ => return Vec::new(),
    };
    let rest = store_name
        .find(marker)
        .map(|i| &store_name[i + marker.len()..])
        .unwrap_or(store_name);
    rest.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

fn store_bin_smokes(path: &Path, bin: &str) -> bool {
    let args: &[&str] = match bin {
        "python3" => &["-c", "print(1)"],
        "node" => &["-e", "process.exit(0)"],
        _ => return true,
    };
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    run_captured_os(
        path,
        &argv,
        &[],
        Duration::from_secs(8),
        None,
        &["LD_PRELOAD"],
    )
    .map(|c| c.rc == 0)
    .unwrap_or(false)
}

fn store_bin(bin: &str) -> Option<PathBuf> {
    let env_key = match bin {
        "python3" => "WORLD_PYTHON3",
        "node" => "WORLD_NODE",
        _ => return None,
    };
    if let Ok(p) = std::env::var(env_key) {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
        let cand = pb.join("bin").join(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let Ok(rd) = fs::read_dir("/nix/store") else {
        return None;
    };
    let mut cands: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !store_pkg_ok(&name, bin) {
            continue;
        }
        let p = e.path().join("bin").join(bin);
        if !p.is_file() {
            continue;
        }
        cands.push((store_version_nums(&name, bin), p));
    }
    cands.sort_by(|a, b| b.0.cmp(&a.0));
    cands
        .into_iter()
        .find(|(_, p)| store_bin_smokes(p, bin))
        .map(|(_, p)| p)
}

fn resolve_bin(name: &str) -> Option<PathBuf> {
    for dir in nix_bin_dirs() {
        let p = dir.join(name);
        if p.is_file() && !is_hidden_bin(&p) {
            return Some(p);
        }
    }
    if let Some(p) = crate::which(name).filter(|p| !is_hidden_bin(p)) {
        return Some(p);
    }
    store_bin(name)
}

fn nix_path() -> OsString {
    let mut parts: Vec<String> = nix_bin_dirs()
        .into_iter()
        .filter(|d| d.is_dir())
        .map(|d| d.display().to_string())
        .collect();
    if let Ok(p) = std::env::var("PATH") {
        parts.push(p);
    }
    parts.join(":").into()
}

fn nixos_config_root() -> PathBuf {
    std::env::var_os("NIXOS_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/nixos"))
}

fn config_blob(root: &Path) -> String {
    let mut out = String::new();
    let Ok(walk) = fs::read_dir(root) else {
        return out;
    };
    let mut stack: Vec<PathBuf> = walk.flatten().map(|e| e.path()).collect();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(rd) = fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("nix") {
            continue;
        }
        if let Ok(s) = fs::read_to_string(&p) {
            out.push_str(&s);
            out.push('\n');
        }
    }
    out
}

fn config_mentions(blob: &str, pkgs: &[&str]) -> bool {
    pkgs.iter().any(|pkg| {
        blob.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .any(|tok| tok == *pkg)
    })
}

fn companion_so_names(file_name: &str) -> &'static [&'static str] {
    if file_name.contains("secure") {
        &["libmimalloc.so", "libmimalloc.so.3"]
    } else {
        &["libmimalloc-secure.so.3", "libmimalloc-secure.so"]
    }
}

fn preload_sos(so: &Path) -> Result<Vec<PathBuf>> {
    let abs = so
        .canonicalize()
        .with_context(|| format!("canonicalize {}", so.display()))?;
    let mut out = vec![abs.clone()];
    let Some(dir) = abs.parent() else {
        return Ok(out);
    };
    let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    for extra in companion_so_names(name) {
        let p = dir.join(extra);
        if !p.exists() {
            continue;
        }
        let c = p.canonicalize().unwrap_or(p);
        if !out.iter().any(|e| e == &c) {
            out.push(c);
        }
    }
    Ok(out)
}

fn write_preload_file(work: &Path, so: Option<&Path>) -> Result<PathBuf> {
    let path = work.join("ld-nix.so.preload");
    if let Some(so) = so {
        let mut text = String::new();
        for p in preload_sos(so)? {
            text.push_str(&p.display().to_string());
            text.push('\n');
        }
        fs::write(&path, text)?;
    } else {
        fs::write(&path, "")?;
    }
    Ok(path)
}

fn run_under_alloc(
    so: Option<&Path>,
    work: &Path,
    program: &Path,
    args: &[OsString],
    extra_env: &[(OsString, OsString)],
    timeout: Duration,
    path_prefix: Option<&Path>,
) -> Result<Captured> {
    let preload_file = write_preload_file(work, so)?;
    let mut env = extra_env.to_vec();
    env.push((OsString::from("HOME"), work.join("home").into_os_string()));
    env.push((
        OsString::from("XDG_RUNTIME_DIR"),
        work.join("run").into_os_string(),
    ));
    fs::create_dir_all(work.join("run"))?;
    env.push((OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")));
    env.push((
        OsString::from("GIT_AUTHOR_DATE"),
        OsString::from("2020-01-01T00:00:00Z"),
    ));
    env.push((
        OsString::from("GIT_COMMITTER_DATE"),
        OsString::from("2020-01-01T00:00:00Z"),
    ));
    let mut path = OsString::new();
    if let Some(p) = path_prefix {
        path.push(p);
        path.push(":");
    }
    path.push(nix_path());
    env.push((OsString::from("PATH"), path));
    if let Ok(tc) = std::env::var("RUSTUP_TOOLCHAIN") {
        env.push((OsString::from("RUSTUP_TOOLCHAIN"), OsString::from(tc)));
    }

    let remove = ["LD_PRELOAD", "WAYLAND_DISPLAY", "DISPLAY"];

    if let Some(bwrap) = find_bwrap() {
        let dests = preload_bind_dests();
        let mut argv: Vec<OsString> = vec![
            "--bind".into(),
            "/".into(),
            "/".into(),
            "--dev-bind".into(),
            "/dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--die-with-parent".into(),
        ];
        for d in dests {
            argv.push("--ro-bind".into());
            argv.push(preload_file.clone().into());
            argv.push(d.into());
        }
        argv.push(program.as_os_str().to_os_string());
        argv.extend(args.iter().cloned());
        return run_captured_os(&bwrap, &argv, &env, timeout, Some(work), &remove);
    }

    if let Some(so) = so {
        let mut joined = OsString::new();
        for (i, p) in preload_sos(so)?.into_iter().enumerate() {
            if i > 0 {
                joined.push(":");
            }
            joined.push(p);
        }
        env.push((OsString::from("LD_PRELOAD"), joined));
    }
    run_captured_os(program, args, &env, timeout, Some(work), &remove)
}

fn run_probe(probe: &Probe, so: Option<&Path>, root: &Path) -> Result<Captured> {
    let bin = resolve_bin(probe.bin).with_context(|| format!("missing {}", probe.bin))?;
    let work = root.join(probe.name.replace(':', "-"));
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(work.join("home"))?;
    let prefix = bin.parent();
    let timeout = Duration::from_secs(probe.timeout_secs);
    match probe.body {
        Body::Args(args) => {
            let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
            run_under_alloc(so, &work, &bin, &argv, &[], timeout, prefix)
        }
        Body::Script(script) => {
            let sh = work.join("probe.sh");
            fs::write(&sh, format!("#!/usr/bin/env bash\n{script}"))?;
            let bash = resolve_bin("bash").unwrap_or_else(|| PathBuf::from("/bin/bash"));
            run_under_alloc(
                so,
                &work,
                &bash,
                &[sh.into_os_string()],
                &[],
                timeout,
                prefix,
            )
        }
    }
}

fn summarize(cap: &Captured) -> String {
    let err = cap.stderr_str();
    let err = err.lines().next().unwrap_or("").trim();
    if err.is_empty() {
        format!("rc={}", cap.rc)
    } else {
        format!(
            "rc={} err={}",
            cap.rc,
            err.chars().take(80).collect::<String>()
        )
    }
}

pub fn run() -> Result<()> {
    let (rust_so, _) = build_mimalloc_cdylibs()?;
    let c_so = crate::oracle::c_mimalloc_secure_so().ok();
    let out = rust_root().join("target/world-preload");
    fs::create_dir_all(&out)?;

    println!("==> world package tests under NixOS preload injection");
    println!("rewrite so: {}", rust_so.display());
    crate::process::check_glibc_cdylib_preload(&rust_so)?;
    match &c_so {
        Some(p) => println!("C so:      {}", p.display()),
        None => println!("C so:      (unavailable; rewrite must still match libc)"),
    }
    println!(
        "note: preload lists libmimalloc.so and libmimalloc-secure.so.3 so DT_NEEDED of nixpkgs mold binds the rewrite, not C mimalloc"
    );

    let cfg_root = nixos_config_root();
    let blob = config_blob(&cfg_root);
    if blob.is_empty() {
        println!(
            "NIXOS_CONFIG {} unreadable; using installed PATH",
            cfg_root.display()
        );
    } else {
        println!("NIXOS_CONFIG {}", cfg_root.display());
    }

    let mut ran = 0usize;
    let mut pass = 0usize;
    let mut skip = 0usize;
    let mut fail_rewrite = 0usize;
    let mut fail_both = 0usize;
    let mut covered = 0usize;
    let mut in_config = 0usize;

    for probe in PROBES {
        let in_cfg = !blob.is_empty() && config_mentions(&blob, probe.pkgs);
        if in_cfg {
            in_config += 1;
        }
        let Some(_) = resolve_bin(probe.bin) else {
            println!(
                "skip {} (no {} on PATH{})",
                probe.name,
                probe.bin,
                if in_cfg {
                    ", listed in NIXOS_CONFIG"
                } else {
                    ""
                }
            );
            skip += 1;
            continue;
        };
        if in_cfg {
            covered += 1;
        }

        print!(".. {} libc", probe.name);
        let libc = run_probe(probe, None, &out.join("libc"))?;
        if libc.rc != 0 {
            println!(" skip ({})", summarize(&libc));
            skip += 1;
            continue;
        }
        print!(" rewrite");
        let rust = run_probe(probe, Some(&rust_so), &out.join("rewrite"))?;
        let c_cap = match &c_so {
            Some(so) => {
                print!(" C");
                Some(run_probe(probe, Some(so), &out.join("c"))?)
            }
            None => None,
        };
        ran += 1;
        let v = judge(&libc, &rust, c_cap.as_ref());
        match v {
            Verdict::Pass => {
                if let Some(c) = &c_cap {
                    if outputs_match(&libc, c) {
                        println!(" ok (C matched)");
                    } else {
                        println!(" ok (C mismatched libc; rewrite matched)");
                    }
                } else {
                    println!(" ok");
                }
                pass += 1;
            }
            Verdict::Skip => {
                println!(" skip ({})", summarize(&libc));
                skip += 1;
            }
            Verdict::FailRewrite => {
                println!(
                    " FAIL rewrite-only libc={} rewrite={}",
                    summarize(&libc),
                    summarize(&rust)
                );
                fail_rewrite += 1;
            }
            Verdict::FailBoth => {
                println!(" note: rewrite and C both mismatch libc ({})", probe.name);
                fail_both += 1;
            }
        }
    }

    println!(
        "world: ran={ran} pass={pass} skip={skip} fail-rewrite={fail_rewrite} fail-both={fail_both} config-probes={in_config} config-covered={covered}"
    );
    if ran == 0 {
        bail!("no world probes ran");
    }
    if fail_rewrite > 0 {
        bail!("{fail_rewrite} rewrite-only world probe failure(s)");
    }
    println!("world-preload: {pass} probes ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::Captured;

    #[test]
    fn probes_have_unique_names() {
        let mut names: Vec<_> = PROBES.iter().map(|p| p.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PROBES.len());
        assert!(PROBES.len() > 10);
    }

    #[test]
    fn cursor_bundled_rg_is_skipped() {
        assert!(is_cursor_tree(Path::new(
            "/nix/store/abc-cursor-1.0/lib/cursor/resources/app/node_modules/@vscode/ripgrep/bin/rg"
        )));
        assert!(!is_cursor_tree(Path::new(
            "/nix/store/abc-ripgrep-14.0/bin/rg"
        )));
        assert!(is_hidden_bin(Path::new("/run/wrappers/bin/kwin_wayland")));
    }

    #[test]
    fn companion_secure_soname_is_paired() {
        assert!(companion_so_names("libmimalloc.so").contains(&"libmimalloc-secure.so.3"));
        assert!(companion_so_names("libmimalloc-secure.so").contains(&"libmimalloc.so"));
    }

    #[test]
    fn store_python_and_node_filters() {
        assert!(store_pkg_ok("3n4q-python3-3.14.7", "python3"));
        assert!(!store_pkg_ok("3n4q-python3-3.14.7-env", "python3"));
        assert!(!store_pkg_ok("3n4q-python3-3.14.7-debug", "python3"));
        assert!(store_pkg_ok("abc-nodejs-22.23.2", "node"));
        assert!(!store_pkg_ok("abc-nodejs-slim-22.23.2", "node"));
        assert!(
            store_version_nums("zzzz-python3-3.14.7", "python3")
                > store_version_nums("aaaa-python3-3.13.14", "python3")
        );
        assert!(
            store_version_nums("aaaa-python3-3.14.7", "python3")
                > store_version_nums("zzzz-python3-3.9.21", "python3")
        );
        assert!(
            store_version_nums("hash-nodejs-22.23.2", "node")
                > store_version_nums("hash-nodejs-18.20.0", "node")
        );
    }

    #[test]
    fn required_world_probes_exist() {
        let names: Vec<_> = PROBES.iter().map(|p| p.name).collect();
        for n in [
            "python3:stdlib-slice",
            "nodejs:buffer-alloc",
            "mold:link",
            "kwin:virtual-session",
            "plasmashell:version",
            "kreadconfig6:roundtrip",
        ] {
            assert!(names.contains(&n), "missing {n}");
        }
    }

    #[test]
    fn config_token_split() {
        let blob = "environment.systemPackages = with pkgs; [ git openssl ];\n";
        assert!(config_mentions(blob, &["git"]));
        assert!(config_mentions(blob, &["openssl"]));
        assert!(!config_mentions(blob, &["python3"]));
    }

    #[test]
    fn rewrite_only_mismatch_is_fail() {
        let libc = Captured::from_utf8_lossy_parts("ok\n", "", 0);
        let rust = Captured::from_utf8_lossy_parts("", "segfault\n", 139);
        let c = Captured::from_utf8_lossy_parts("ok\n", "", 0);
        assert_eq!(judge(&libc, &rust, Some(&c)), Verdict::FailRewrite);
        assert_eq!(judge(&libc, &c, Some(&c)), Verdict::Pass);
    }

    #[test]
    fn matching_c_mismatch_is_not_rewrite_regression() {
        let libc = Captured::from_utf8_lossy_parts("ok\n", "", 0);
        let bad = Captured::from_utf8_lossy_parts("", "", 139);
        assert_eq!(judge(&libc, &bad, Some(&bad)), Verdict::FailBoth);
    }

    #[test]
    fn libc_nonzero_skips() {
        let libc = Captured::from_utf8_lossy_parts("", "missing\n", 1);
        let rust = Captured::from_utf8_lossy_parts("ok\n", "", 0);
        assert_eq!(judge(&libc, &rust, None), Verdict::Skip);
    }
}
