//! Firefox, Chromium, and Electron as process-allocator smokes.
//!
//! Compiler-suite `LD_PRELOAD` is not a substitute. These apps must start,
//! spawn child processes that also map the allocator, and complete a short
//! headless page load. Injection matches NixOS `memoryAllocator`: bind a
//! preload file over `/etc/ld-nix.so.preload` (content processes can drop
//! `LD_PRELOAD`). Hide-system-malloc PATH wraps are unwrapped via `real=`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use wait_timeout::ChildExt;

use crate::process::{build_mimalloc_cdylibs, status_code};
use crate::rust_root;

const MARKER: &str = "mimalloc-ok";
const MAPS_NEEDLE: &str = "libmimalloc";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppKind {
    Firefox,
    Chromium,
    Electron,
}

impl AppKind {
    fn label(self) -> &'static str {
        match self {
            AppKind::Firefox => "firefox",
            AppKind::Chromium => "chromium",
            AppKind::Electron => "electron",
        }
    }
}

#[derive(Clone, Debug)]
pub struct App {
    pub kind: AppKind,
    pub launcher: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SmokeReport {
    pub alloc: String,
    pub rc: i32,
    pub smoke_ok: bool,
    pub parent_mi: bool,
    pub child_mi: bool,
    pub children_seen: usize,
    pub via_preload_file: bool,
    pub stdout: String,
    pub stderr: String,
}

impl SmokeReport {
    fn recipe_ok(&self) -> bool {
        self.smoke_ok && self.rc == 0 && self.children_seen > 0
    }

    fn alloc_ok(&self) -> bool {
        self.recipe_ok() && self.parent_mi && self.child_mi
    }
}

/// `real="/nix/store/..."` from hide-system-malloc wrap scripts.
pub fn parse_real_eq(script: &str) -> Option<String> {
    for line in script.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("real=") else {
            continue;
        };
        let rest = rest.trim().trim_matches('"').trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

/// Follow hide-system-malloc `real=` wrappers; keep nixpkgs env wrappers.
pub fn unwrap_launcher(path: &Path) -> PathBuf {
    let mut cur = path.to_path_buf();
    for _ in 0..8 {
        let Ok(data) = fs::read(&cur) else {
            return cur;
        };
        if data.starts_with(b"\x7fELF") {
            return cur;
        }
        let text = String::from_utf8_lossy(&data);
        match parse_real_eq(&text) {
            Some(next) => cur = PathBuf::from(next),
            None => return cur,
        }
    }
    cur
}

pub fn parse_ppid_stat(stat: &str) -> Option<u32> {
    let rparen = stat.rfind(')')?;
    let mut fields = stat[rparen + 1..].split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

fn proc_ppids() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let Ok(rd) = fs::read_dir("/proc") else {
        return map;
    };
    for e in rd.flatten() {
        let pid: u32 = match e.file_name().to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let Ok(stat) = fs::read_to_string(e.path().join("stat")) else {
            continue;
        };
        if let Some(ppid) = parse_ppid_stat(&stat) {
            map.insert(pid, ppid);
        }
    }
    map
}

fn descendants_of(root: u32) -> Vec<u32> {
    let ppids = proc_ppids();
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &ppid) in &ppids {
        kids.entry(ppid).or_default().push(pid);
    }
    let mut out = Vec::new();
    let mut q = VecDeque::from([root]);
    let mut seen = HashSet::from([root]);
    while let Some(p) = q.pop_front() {
        out.push(p);
        if let Some(cs) = kids.get(&p) {
            for &c in cs {
                if seen.insert(c) {
                    q.push_back(c);
                }
            }
        }
    }
    out
}

fn maps_have_so(pid: u32, so: &Path) -> bool {
    let Ok(maps) = fs::read_to_string(format!("/proc/{pid}/maps")) else {
        return false;
    };
    if maps.contains(MAPS_NEEDLE) {
        let name = so.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.is_empty() && maps.contains(name) {
            return true;
        }
        if let Ok(canon) = so.canonicalize() {
            if maps.contains(&*canon.to_string_lossy()) {
                return true;
            }
        }
        return maps.contains(MAPS_NEEDLE);
    }
    false
}

fn sample_tree(root: u32, so: Option<&Path>) -> (usize, usize) {
    let pids = descendants_of(root);
    let children = pids.iter().filter(|p| **p != root).count();
    let mi = match so {
        Some(so) => pids.iter().filter(|p| maps_have_so(**p, so)).count(),
        None => 0,
    };
    (children, mi)
}

fn kill_tree(root: u32) {
    let mut pids = descendants_of(root);
    pids.reverse();
    for pid in pids {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

/// Absolute `.../bin/bwrap` from hide-system-malloc wrap scripts.
pub fn parse_embedded_bwrap(script: &str) -> Option<String> {
    for line in script.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("exec ") else {
            continue;
        };
        let path = rest.split_whitespace().next()?;
        if path.ends_with("/bin/bwrap") {
            return Some(path.to_string());
        }
    }
    None
}

fn find_bwrap() -> Option<PathBuf> {
    if let Some(p) = env_path("BWRAP").or_else(|| crate::which("bwrap")) {
        return Some(p);
    }
    for name in ["firefox", "microsoft-edge", "microsoft-edge-stable", "signal-desktop"] {
        let Some(p) = crate::which(name) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        if let Some(b) = parse_embedded_bwrap(&text) {
            let pb = PathBuf::from(&b);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    None
}

fn which_first(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|n| crate::which(n))
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.exists())
}

pub fn discover_apps() -> Result<Vec<App>> {
    let mut apps = Vec::new();
    let firefox = env_path("FIREFOX").or_else(|| which_first(&["firefox"]));
    let chromium = env_path("CHROMIUM").or_else(|| {
        which_first(&[
            "chromium",
            "google-chrome-stable",
            "google-chrome",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
        ])
    });
    let electron = env_path("ELECTRON").or_else(|| which_first(&["electron"]));

    if let Some(p) = firefox {
        apps.push(App {
            kind: AppKind::Firefox,
            launcher: unwrap_launcher(&p),
        });
    }
    if let Some(p) = chromium {
        apps.push(App {
            kind: AppKind::Chromium,
            launcher: unwrap_launcher(&p),
        });
    }
    if let Some(p) = electron {
        apps.push(App {
            kind: AppKind::Electron,
            launcher: unwrap_launcher(&p),
        });
    }
    if apps.is_empty() {
        bail!(
            "no Firefox/Chromium/Electron on PATH (set FIREFOX, CHROMIUM, ELECTRON to unwrapped launchers)"
        );
    }
    Ok(apps)
}

fn timeout_secs() -> u64 {
    std::env::var("BROWSER_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90)
}

fn preload_bind_dests() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let candidates = [
        PathBuf::from("/etc/ld-nix.so.preload"),
        PathBuf::from("/etc/static/ld-nix.so.preload"),
    ];
    for c in candidates {
        if c.exists() {
            if let Ok(real) = fs::canonicalize(&c) {
                if real.is_file() && !out.contains(&real) {
                    out.push(real);
                }
            }
            if c.is_file() && !c.is_symlink() && !out.contains(&c) {
                out.push(c);
            }
        } else if !out.contains(&c) {
            out.push(c);
        }
    }
    if out.is_empty() {
        out.push(PathBuf::from("/etc/ld-nix.so.preload"));
    }
    out
}

fn png_ok(path: &Path) -> bool {
    let Ok(b) = fs::read(path) else {
        return false;
    };
    b.len() > 64 && b.starts_with(b"\x89PNG")
}

fn write_page(dir: &Path) -> Result<PathBuf> {
    let page = dir.join("page.html");
    fs::write(
        &page,
        format!("<html><body>{MARKER}</body></html>\n"),
    )?;
    Ok(page)
}

fn electron_main(dir: &Path) -> Result<PathBuf> {
    let page = write_page(dir)?;
    let url = format!("file://{}", page.display());
    let main = dir.join("main.js");
    fs::write(
        &main,
        format!(
            r#"const {{ app, BrowserWindow }} = require("electron");
app.commandLine.appendSwitch("no-sandbox");
app.commandLine.appendSwitch("disable-gpu");
app.whenReady().then(() => {{
  const w = new BrowserWindow({{
    show: false,
    width: 800,
    height: 600,
    webPreferences: {{ sandbox: false }},
  }});
  w.loadURL({url:?});
  w.webContents.on("did-finish-load", () => {{
    w.webContents.executeJavaScript("document.body.innerText").then((t) => {{
      process.stdout.write(String(t).trim() + "\n");
      app.exit(String(t).trim() === {MARKER:?} ? 0 : 2);
    }}).catch((e) => {{
      console.error(e);
      app.exit(3);
    }});
  }});
}});
setTimeout(() => app.exit(124), 25000);
"#
        ),
    )?;
    Ok(main)
}

struct SpawnSpec {
    program: PathBuf,
    args: Vec<OsString>,
    extra_env: Vec<(OsString, OsString)>,
    via_bwrap: bool,
}

fn spawn_spec(
    so: Option<&Path>,
    launcher: &Path,
    args: &[&str],
    work: &Path,
    env: &[(&str, OsString)],
) -> Result<SpawnSpec> {
    let mut extra_env: Vec<(OsString, OsString)> = env
        .iter()
        .map(|(k, v)| (OsString::from(*k), v.clone()))
        .collect();
    extra_env.push((
        OsString::from("HOME"),
        work.join("home").into_os_string(),
    ));
    extra_env.push((
        OsString::from("XDG_CONFIG_HOME"),
        work.join("config").into_os_string(),
    ));
    extra_env.push((
        OsString::from("XDG_CACHE_HOME"),
        work.join("cache").into_os_string(),
    ));
    extra_env.push((
        OsString::from("XDG_DATA_HOME"),
        work.join("data").into_os_string(),
    ));
    extra_env.push((OsString::from("NO_AT_BRIDGE"), OsString::from("1")));
    extra_env.push((OsString::from("MOZ_NO_REMOTE"), OsString::from("1")));
    extra_env.push((OsString::from("MOZ_CRASHREPORTER_DISABLE"), OsString::from("1")));
    extra_env.push((OsString::from("MOZ_DISABLE_CRASH_REPORTER"), OsString::from("1")));
    fs::create_dir_all(work.join("home"))?;
    fs::create_dir_all(work.join("config"))?;
    fs::create_dir_all(work.join("cache"))?;
    fs::create_dir_all(work.join("data"))?;

    let preload_file = work.join("ld-nix.so.preload");
    if let Some(so) = so {
        let so_abs = so
            .canonicalize()
            .with_context(|| format!("canonicalize {}", so.display()))?;
        fs::write(&preload_file, format!("{}\n", so_abs.display()))?;
    } else {
        fs::write(&preload_file, "")?;
    }

    if let Some(bwrap) = find_bwrap() {
        let dests = preload_bind_dests();
        let mut args_os: Vec<OsString> = vec![
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
            args_os.push("--ro-bind".into());
            args_os.push(preload_file.clone().into());
            args_os.push(d.into());
        }
        args_os.push(launcher.as_os_str().to_os_string());
        for a in args {
            args_os.push((*a).into());
        }
        return Ok(SpawnSpec {
            program: bwrap,
            args: args_os,
            extra_env,
            via_bwrap: true,
        });
    }

    if let Some(so) = so {
        extra_env.push((
            OsString::from("LD_PRELOAD"),
            so.canonicalize()
                .with_context(|| format!("canonicalize {}", so.display()))?
                .into_os_string(),
        ));
    }
    Ok(SpawnSpec {
        program: launcher.to_path_buf(),
        args: args.iter().map(|a| OsString::from(*a)).collect(),
        extra_env,
        via_bwrap: false,
    })
}

fn run_smoke(app: &App, so: Option<&Path>, alloc: &str, work: &Path) -> Result<SmokeReport> {
    let timeout = Duration::from_secs(timeout_secs());
    let page = write_page(work)?;
    let page_url = format!("file://{}", page.display());
    let shot = work.join("screenshot.png");
    let dump = work.join("dump.html");
    let profile = work.join("profile");
    fs::create_dir_all(&profile)?;

    let mut extra: Vec<(&str, OsString)> = Vec::new();
    let arg_store: Vec<String>;
    let args: Vec<&str> = match app.kind {
        AppKind::Firefox => {
            extra.push(("MOZ_HEADLESS", OsString::from("1")));
            extra.push(("MOZ_DISABLE_CONTENT_SANDBOX", OsString::from("1")));
            arg_store = vec![
                "--headless".into(),
                "--new-instance".into(),
                "--profile".into(),
                profile.to_string_lossy().into_owned(),
                format!("--screenshot={}", shot.display()),
                "--window-size=800,600".into(),
                page_url.clone(),
            ];
            arg_store.iter().map(String::as_str).collect()
        }
        AppKind::Chromium => {
            arg_store = vec![
                "--headless=new".into(),
                "--ozone-platform=headless".into(),
                "--disable-gpu".into(),
                "--no-sandbox".into(),
                "--disable-dev-shm-usage".into(),
                "--no-first-run".into(),
                "--no-default-browser-check".into(),
                "--disable-extensions".into(),
                "--disable-crash-reporter".into(),
                format!("--user-data-dir={}", profile.display()),
                format!("--crash-dumps-dir={}", work.join("crashes").display()),
                "--dump-dom".into(),
                page_url.clone(),
            ];
            arg_store.iter().map(String::as_str).collect()
        }
        AppKind::Electron => {
            let main = electron_main(work)?;
            arg_store = vec![
                "--no-sandbox".into(),
                "--disable-gpu".into(),
                "--disable-crash-reporter".into(),
                main.to_string_lossy().into_owned(),
            ];
            arg_store.iter().map(String::as_str).collect()
        }
    };

    let spec = spawn_spec(so, &app.launcher, &args, work, &extra)?;
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(work);
    for (k, v) in &spec.extra_env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn {} via {}", app.kind.label(), spec.program.display()))?;
    let root = child.id();
    let mut stdout_p = child.stdout.take().expect("stdout");
    let mut stderr_p = child.stderr.take().expect("stderr");
    let t_out = thread::spawn(move || {
        use std::io::Read;
        let mut b = Vec::new();
        stdout_p.read_to_end(&mut b).ok();
        b
    });
    let t_err = thread::spawn(move || {
        use std::io::Read;
        let mut b = Vec::new();
        stderr_p.read_to_end(&mut b).ok();
        b
    });

    let start = Instant::now();
    let mut parent_mi = false;
    let mut child_mi = false;
    let mut children_seen = 0usize;
    let status = loop {
        let (n, mi) = sample_tree(root, so);
        parent_mi |= mi >= 1;
        child_mi |= mi >= 2;
        children_seen = children_seen.max(n);
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if start.elapsed() > timeout {
                    kill_tree(root);
                    let _ = child.kill();
                    let _ = child.wait_timeout(Duration::from_secs(2));
                    let _ = child.kill();
                    let stdout = t_out.join().unwrap_or_default();
                    let stderr = t_err.join().unwrap_or_default();
                    return Ok(SmokeReport {
                        alloc: alloc.to_string(),
                        rc: 124,
                        smoke_ok: false,
                        parent_mi,
                        child_mi,
                        children_seen,
                        via_preload_file: spec.via_bwrap,
                        stdout: String::from_utf8_lossy(&stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                    });
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(_) => {
                kill_tree(root);
                let stdout = t_out.join().unwrap_or_default();
                let stderr = t_err.join().unwrap_or_default();
                return Ok(SmokeReport {
                    alloc: alloc.to_string(),
                    rc: 255,
                    smoke_ok: false,
                    parent_mi,
                    child_mi,
                    children_seen,
                    via_preload_file: spec.via_bwrap,
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                });
            }
        }
    };

    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    let stdout_s = String::from_utf8_lossy(&stdout).into_owned();
    let stderr_s = String::from_utf8_lossy(&stderr).into_owned();
    if app.kind == AppKind::Chromium {
        let _ = fs::write(&dump, &stdout);
    }

    let smoke_ok = match app.kind {
        AppKind::Firefox => png_ok(&shot) || png_ok(&work.join("screenshot.png")),
        AppKind::Chromium => stdout_s.contains(MARKER),
        AppKind::Electron => stdout_s.contains(MARKER) && status_code(status) == 0,
    };

    Ok(SmokeReport {
        alloc: alloc.to_string(),
        rc: status_code(status),
        smoke_ok,
        parent_mi,
        child_mi,
        children_seen,
        via_preload_file: spec.via_bwrap,
        stdout: stdout_s,
        stderr: stderr_s,
    })
}

fn print_report(app: &App, r: &SmokeReport) {
    println!(
        "  {:<10} {alloc:<12} rc={rc} smoke={smoke} parent_mi={parent} children={n} child_mi={child} preload_file={via}",
        app.kind.label(),
        alloc = r.alloc,
        rc = r.rc,
        smoke = r.smoke_ok,
        parent = r.parent_mi,
        n = r.children_seen,
        child = r.child_mi,
        via = r.via_preload_file,
    );
    if !r.recipe_ok() || (r.alloc != "libc" && !r.alloc_ok()) {
        let err = r.stderr.chars().take(2000).collect::<String>();
        let out = r.stdout.chars().take(500).collect::<String>();
        if !out.is_empty() {
            println!("    stdout: {}", out.replace('\n', " "));
        }
        if !err.is_empty() {
            println!("    stderr: {}", err.replace('\n', " | "));
        }
    }
}

fn judge(
    app: &App,
    control: &SmokeReport,
    rust: &SmokeReport,
    c: Option<&SmokeReport>,
) -> Result<()> {
    if !control.recipe_ok() {
        bail!(
            "{} libc control failed (rc={} smoke={} children={}); smoke recipe is broken",
            app.kind.label(),
            control.rc,
            control.smoke_ok,
            control.children_seen
        );
    }
    if rust.alloc_ok() {
        if let Some(c) = c {
            if !c.alloc_ok() {
                println!(
                    "  note: C mimalloc failed {} (rc={} smoke={}) while rewrite passed",
                    app.kind.label(),
                    c.rc,
                    c.smoke_ok
                );
            }
        }
        return Ok(());
    }
    if let Some(c) = c {
        if c.alloc_ok() {
            bail!(
                "rewrite failed {} while C mimalloc passed (rc={} smoke={} parent_mi={} child_mi={} children={})",
                app.kind.label(),
                rust.rc,
                rust.smoke_ok,
                rust.parent_mi,
                rust.child_mi,
                rust.children_seen
            );
        }
        println!(
            "  note: {} cannot use mimalloc via ld.so.preload (rewrite rc={} C rc={}; matches C)",
            app.kind.label(),
            rust.rc,
            c.rc
        );
        return Ok(());
    }
    bail!(
        "rewrite failed {} rc={} smoke={} parent_mi={} child_mi={} children={} (no C oracle)",
        app.kind.label(),
        rust.rc,
        rust.smoke_ok,
        rust.parent_mi,
        rust.child_mi,
        rust.children_seen
    );
}

pub fn run() -> Result<()> {
    println!("==> browsers: Firefox / Chromium / Electron vs C mimalloc");
    let (rust_so, rust_secure) = build_mimalloc_cdylibs()?;
    let so = if crate::env_is_one("BROWSER_SECURE") {
        rust_secure
    } else {
        rust_so
    };
    println!("rewrite so: {}", so.display());

    let c_so = match crate::oracle::c_mimalloc_secure_so() {
        Ok(p) => {
            println!("C so:      {}", p.display());
            Some(p)
        }
        Err(e) => {
            println!("skip C mimalloc oracle ({e:#})");
            None
        }
    };

    let apps = discover_apps()?;
    for a in &apps {
        println!(
            "  found {} -> {}",
            a.kind.label(),
            a.launcher.display()
        );
    }
    let kinds: HashSet<_> = apps.iter().map(|a| a.kind).collect();
    for need in [AppKind::Firefox, AppKind::Chromium, AppKind::Electron] {
        if !kinds.contains(&need) && !crate::env_is_one("BROWSER_ALLOW_PARTIAL") {
            bail!(
                "missing {} (set {} to an unwrapped launcher, or BROWSER_ALLOW_PARTIAL=1)",
                need.label(),
                need.label().to_uppercase()
            );
        }
    }

    let cache = rust_root().join("target/browsers");
    fs::create_dir_all(&cache)?;
    let mut failed = false;
    for app in &apps {
        let ctl_dir = cache.join(format!("{}-libc", app.kind.label()));
        let _ = fs::remove_dir_all(&ctl_dir);
        fs::create_dir_all(&ctl_dir)?;
        println!("==> {} libc (control)", app.kind.label());
        let control = run_smoke(app, None, "libc", &ctl_dir)?;
        print_report(app, &control);

        let rust_dir = cache.join(format!("{}-rewrite", app.kind.label()));
        let _ = fs::remove_dir_all(&rust_dir);
        fs::create_dir_all(&rust_dir)?;
        println!("==> {} rewrite", app.kind.label());
        let rust = run_smoke(app, Some(&so), "rewrite", &rust_dir)?;
        print_report(app, &rust);

        let c_rep = if let Some(ref c_so) = c_so {
            let c_dir = cache.join(format!("{}-c", app.kind.label()));
            let _ = fs::remove_dir_all(&c_dir);
            fs::create_dir_all(&c_dir)?;
            println!("==> {} C mimalloc", app.kind.label());
            let r = run_smoke(app, Some(c_so), "c-mimalloc", &c_dir)?;
            print_report(app, &r);
            Some(r)
        } else {
            None
        };

        if let Err(e) = judge(app, &control, &rust, c_rep.as_ref()) {
            eprintln!("{e:#}");
            failed = true;
        } else {
            println!("ok {}", app.kind.label());
        }
    }

    if failed {
        bail!("browser allocator smokes failed");
    }
    println!("browsers: {} app(s) ok under rewrite", apps.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hide_system_malloc_real() {
        let s = r#"#!/bin/bash
empty=/nix/store/empty-ld-nix.so.preload
real="/nix/store/abc-firefox-154.0/bin/firefox"

if [ ! -e /etc/ld-nix.so.preload ]; then
  exec "$real" "$@"
fi
"#;
        assert_eq!(
            parse_real_eq(s).as_deref(),
            Some("/nix/store/abc-firefox-154.0/bin/firefox")
        );
    }

    #[test]
    fn parse_ppid_from_stat() {
        let stat = "1234 (firefox) S 1000 1234 1000 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0";
        assert_eq!(parse_ppid_stat(stat), Some(1000));
        let comm_paren = "99 (some)name) S 1 99 1 0";
        assert_eq!(parse_ppid_stat(comm_paren), Some(1));
    }

    #[test]
    fn kinds_cover_backlog() {
        assert_eq!(AppKind::Firefox.label(), "firefox");
        assert_eq!(AppKind::Chromium.label(), "chromium");
        assert_eq!(AppKind::Electron.label(), "electron");
    }

    #[test]
    fn parse_embedded_bwrap_path() {
        let s = r#"exec /nix/store/abc-bubblewrap-0.11.2/bin/bwrap "${args[@]}" "$real" "$@""#;
        assert_eq!(
            parse_embedded_bwrap(s).as_deref(),
            Some("/nix/store/abc-bubblewrap-0.11.2/bin/bwrap")
        );
    }

    fn report(alloc: &str, rc: i32, smoke: bool, children: usize, parent_mi: bool, child_mi: bool) -> SmokeReport {
        SmokeReport {
            alloc: alloc.into(),
            rc,
            smoke_ok: smoke,
            parent_mi,
            child_mi,
            children_seen: children,
            via_preload_file: true,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[test]
    fn matching_c_crash_is_ok_when_control_passed() {
        let app = App {
            kind: AppKind::Firefox,
            launcher: PathBuf::from("/bin/firefox"),
        };
        let control = report("libc", 0, true, 5, false, false);
        let rust = report("rewrite", 139, false, 3, true, true);
        let c = report("c-mimalloc", 139, false, 3, true, true);
        assert!(judge(&app, &control, &rust, Some(&c)).is_ok());
    }

    #[test]
    fn rewrite_crash_vs_c_pass_is_fail() {
        let app = App {
            kind: AppKind::Chromium,
            launcher: PathBuf::from("/bin/chromium"),
        };
        let control = report("libc", 0, true, 8, false, false);
        let rust = report("rewrite", 139, false, 3, true, true);
        let c = report("c-mimalloc", 0, true, 8, true, true);
        assert!(judge(&app, &control, &rust, Some(&c)).is_err());
    }
}
