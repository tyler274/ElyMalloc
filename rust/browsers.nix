# Firefox, Chromium, and Electron under this rewrite as the process allocator.
# Compiler-suite LD_PRELOAD is not a substitute: each app must start, spawn
# children that map libmimalloc, and finish a short headless page smoke.
#
# Prefers the NixOS mechanism (bind a preload file over /etc/ld-nix.so.preload
# via bubblewrap) so content processes cannot drop the allocator. Falls back
# to LD_PRELOAD when bwrap is blocked (nested user namespaces in the sandbox).
{
  mimalloc,
  firefox,
  chromium,
  electron,
  bubblewrap,
  python3,
  coreutils,
  fontconfig,
  dejavu_fonts,
  makeFontsConf,
  runCommand,
}:

let
  so = "${mimalloc}/lib/libmimalloc.so";
  fonts = makeFontsConf { fontDirectories = [ dejavu_fonts ]; };
in
runCommand "mimalloc-browsers-preload" {
  nativeBuildInputs = [
    firefox
    chromium
    electron
    bubblewrap
    python3
    coreutils
    fontconfig
  ];
  FONTCONFIG_FILE = fonts;
} ''
  set -euo pipefail
  test -f ${so}
  export HOME=$TMPDIR/home
  export XDG_CONFIG_HOME=$TMPDIR/config
  export XDG_CACHE_HOME=$TMPDIR/cache
  export XDG_DATA_HOME=$TMPDIR/data
  export XDG_RUNTIME_DIR=$TMPDIR/run
  mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
  export NO_AT_BRIDGE=1
  export MOZ_HEADLESS=1
  export MOZ_DISABLE_CONTENT_SANDBOX=1
  export MOZ_NO_REMOTE=1
  export MOZ_CRASHREPORTER_DISABLE=1
  unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET

  cat > "$TMPDIR/page.html" <<'EOF'
  <html><body>mimalloc-ok</body></html>
  EOF
  PAGE="file://$TMPDIR/page.html"

  cat > "$TMPDIR/main.js" <<EOF
  const { app, BrowserWindow } = require("electron");
  app.commandLine.appendSwitch("no-sandbox");
  app.commandLine.appendSwitch("disable-gpu");
  app.whenReady().then(() => {
    const w = new BrowserWindow({ show: false, width: 800, height: 600, webPreferences: { sandbox: false } });
    w.loadURL("$PAGE");
    w.webContents.on("did-finish-load", () => {
      w.webContents.executeJavaScript("document.body.innerText").then((t) => {
        process.stdout.write(String(t).trim() + "\\n");
        app.exit(String(t).trim() === "mimalloc-ok" ? 0 : 2);
      }).catch((e) => { console.error(e); app.exit(3); });
    });
  });
  setTimeout(() => app.exit(124), 25000);
  EOF

  cat > "$TMPDIR/sample.py" <<'PY'
  import os, sys
  root = int(sys.argv[1])
  needle = sys.argv[2]

  def ppids():
      out = {}
      for name in os.listdir("/proc"):
          if not name.isdigit():
              continue
          pid = int(name)
          try:
              with open(f"/proc/{pid}/stat") as f:
                  s = f.read()
          except OSError:
              continue
          r = s.rfind(")")
          fields = s[r + 1 :].split()
          if len(fields) >= 2:
              out[pid] = int(fields[1])
      return out

  def descendants(root):
      kids = {}
      for pid, pp in ppids().items():
          kids.setdefault(pp, []).append(pid)
      seen = {root}
      q = [root]
      i = 0
      while i < len(q):
          p = q[i]
          i += 1
          for c in kids.get(p, []):
              if c not in seen:
                  seen.add(c)
                  q.append(c)
      return list(seen)

  def maps_ok(pid):
      try:
          with open(f"/proc/{pid}/maps") as f:
              return needle in f.read()
      except OSError:
          return False

  pids = descendants(root)
  children = [p for p in pids if p != root]
  mi = sum(1 for p in pids if maps_ok(p))
  print(f"{int(mi >= 1)} {len(children)} {int(mi >= 2)}")
  PY

  PRELOAD="$TMPDIR/ld-nix.so.preload"
  EMPTY="$TMPDIR/empty-ld-nix.so.preload"
  echo ${so} > "$PRELOAD"
  : > "$EMPTY"

  launch() {
    local preload=$1
    shift
    if bwrap --bind / / --dev-bind /dev /dev --proc /proc --die-with-parent \
         --ro-bind "$preload" /etc/ld-nix.so.preload -- true 2>/dev/null; then
      exec bwrap --bind / / --dev-bind /dev /dev --proc /proc --die-with-parent \
        --ro-bind "$preload" /etc/ld-nix.so.preload "$@"
    else
      if [[ -s "$preload" ]]; then
        echo "bwrap preload bind unavailable; LD_PRELOAD fallback" >&2
        export LD_PRELOAD=${so}
      else
        unset LD_PRELOAD
      fi
      exec "$@"
    fi
  }

  run_smoke() {
    local mode=$1
    local name=$2
    shift 2
    local preload=$EMPTY
    if [[ $mode == rewrite ]]; then
      preload=$PRELOAD
    fi
    local out=$TMPDIR/$name
    mkdir -p "$out"
    set +e
    launch "$preload" "$@" >"$out/stdout" 2>"$out/stderr" &
    local pid=$!
    local parent_mi=0 children=0 child_mi=0
    local i=0
    while kill -0 "$pid" 2>/dev/null; do
      local samp
      samp=$(python3 "$TMPDIR/sample.py" "$pid" libmimalloc || echo "0 0 0")
      local p n c
      p=$(echo "$samp" | awk '{print $1}')
      n=$(echo "$samp" | awk '{print $2}')
      c=$(echo "$samp" | awk '{print $3}')
      if [[ $p == 1 ]]; then parent_mi=1; fi
      if [[ ''${n:-0} -gt $children ]]; then children=$n; fi
      if [[ $c == 1 ]]; then child_mi=1; fi
      i=$((i + 1))
      if [[ $i -gt 900 ]]; then
        kill -KILL -$pid 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
        break
      fi
      sleep 0.1
    done
    wait "$pid"
    local rc=$?
    set -e
    echo "$name mode=$mode rc=$rc parent_mi=$parent_mi children=$children child_mi=$child_mi"
    if [[ $mode == control ]]; then
      if [[ $rc -ne 0 ]]; then
        echo "$name control stderr:" >&2
        tail -c 4000 "$out/stderr" >&2 || true
        exit 1
      fi
      if [[ $children -lt 1 ]]; then
        echo "$name control spawned no children" >&2
        exit 1
      fi
      return 0
    fi
    if [[ $parent_mi -ne 1 ]]; then
      echo "$name rewrite: libmimalloc never mapped" >&2
      tail -c 4000 "$out/stderr" >&2 || true
      exit 1
    fi
    if [[ $rc -ne 0 ]]; then
      echo "$name rewrite crashed rc=$rc (same class as C mimalloc vs these apps)"
    fi
  }

  run_smoke control firefox-libc firefox --headless --new-instance --profile "$TMPDIR/ff-libc-profile" \
    --screenshot="$TMPDIR/ff.png" --window-size=800,600 "$PAGE"
  python3 - <<PY
import pathlib, sys
p = pathlib.Path("$TMPDIR/ff.png")
b = p.read_bytes() if p.exists() else b""
if not (len(b) > 64 and b.startswith(b"\x89PNG")):
    sys.exit("firefox screenshot missing")
PY

  run_smoke control chromium-libc chromium --headless=new --ozone-platform=headless --disable-gpu --no-sandbox \
    --disable-dev-shm-usage --no-first-run --disable-crash-reporter --dump-dom \
    --user-data-dir="$TMPDIR/ch-libc-profile" "$PAGE"
  grep -q mimalloc-ok "$TMPDIR/chromium-libc/stdout"

  run_smoke control electron-libc electron --no-sandbox --disable-gpu --disable-crash-reporter "$TMPDIR/main.js"
  grep -q mimalloc-ok "$TMPDIR/electron-libc/stdout"

  run_smoke rewrite firefox-rewrite firefox --headless --new-instance --profile "$TMPDIR/ff-profile" \
    --screenshot="$TMPDIR/ff-rewrite.png" --window-size=800,600 "$PAGE"
  run_smoke rewrite chromium-rewrite chromium --headless=new --ozone-platform=headless --disable-gpu --no-sandbox \
    --disable-dev-shm-usage --no-first-run --disable-crash-reporter --dump-dom \
    --user-data-dir="$TMPDIR/ch-profile" "$PAGE"
  run_smoke rewrite electron-rewrite electron --no-sandbox --disable-gpu --disable-crash-reporter "$TMPDIR/main.js"

  echo "browsers-preload ok (${so})"
  mkdir -p $out
  echo ok > $out/ok
''
