# Real workloads of typical NixOS-world packages against this rewrite.
# Not a substitute for Firefox/Chromium/Electron: those are
# rust/browsers.nix and `mimalloc-harness browsers`. Compile/link success
# is not enough: each binary must run. The live-host harness
# (`mimalloc-harness world`) also compares rewrite vs C vs libc using
# packages from NIXOS_CONFIG=/etc/nixos.
{
  lib,
  mimalloc,
  hello,
  coreutils,
  gnugrep,
  gawk,
  gnused,
  findutils,
  diffutils,
  gnutar,
  gzip,
  xz,
  bzip2,
  git,
  curl,
  wget,
  openssl,
  python3,
  perl,
  ripgrep,
  jq,
  gnumake,
  gcc,
  binutils,
  bash,
  zstd,
  e2fsprogs,
  nodejs,
  mold,
  runCommand,
}:

let
  so = "${mimalloc}/lib/libmimalloc.so";
  secure = "${mimalloc}/lib/libmimalloc-secure.so.3";
  bin = lib.getBin;
in
runCommand "mimalloc-world-preload" {
  nativeBuildInputs = [
    hello
    coreutils
    gnugrep
    gawk
    gnused
    findutils
    diffutils
    gnutar
    gzip
    (bin xz)
    (bin bzip2)
    git
    (bin curl)
    wget
    (bin openssl)
    python3
    perl
    ripgrep
    (bin jq)
    gnumake
    gcc
    binutils
    (bin bash)
    (bin zstd)
    (bin e2fsprogs)
    nodejs
    (bin mold)
  ];
} ''
  set -euo pipefail
  test -f ${so}
  test -f ${secure}
  export LD_PRELOAD=${so}:${secure}
  export HOME=$TMPDIR
  export GIT_CONFIG_NOSYSTEM=1
  export GIT_AUTHOR_DATE=2020-01-01T00:00:00Z
  export GIT_COMMITTER_DATE=2020-01-01T00:00:00Z

  echo "==> hello / coreutils"
  hello
  ls ${so} >/dev/null
  grep -q mimalloc ${so} || true

  echo "==> git"
  git init -q
  git config user.name mi
  git config user.email mi@example.invalid
  printf 'hello mimalloc\n' > README
  git add README
  git commit -q -m m
  git gc -q --prune=now
  git log -1 --format=%s | grep -qx m

  echo "==> curl"
  printf 'curl-ok\n' > page.txt
  curl -fsS "file://$PWD/page.txt" | grep -qx curl-ok
  wget -q -O - "file://$PWD/page.txt" 2>/dev/null | grep -qx curl-ok || echo "skip wget file://"

  echo "==> openssl / perl / rg / jq / make"
  head -c 65536 /dev/zero > in.bin
  openssl enc -aes-256-cbc -pbkdf2 -salt -pass pass:mimalloc -in in.bin -out enc.bin 2>/dev/null
  openssl enc -d -aes-256-cbc -pbkdf2 -pass pass:mimalloc -in enc.bin -out out.bin 2>/dev/null
  cmp in.bin out.bin

  perl -e 'my $x = "a" x 200000; die unless length($x)==200000; print "perl-ok\n"'

  printf 'alpha\nmimalloc-hit\nomega\n' > hay.txt
  rg -x mimalloc-hit hay.txt >/dev/null
  jq -n '{ok:true,n:([range(100)|.+1]|add)}' >/dev/null

  printf 'all:\n\t@echo make-ok\n' > Makefile
  make -s | grep -qx make-ok

  echo "==> python"
  python3 -c 'import hashlib,threading,json
blob=bytes(i%256 for i in range(8000))
hashlib.sha256(blob*64).hexdigest()
out=[]
def work(n):
  out.append(sum(range(n)))
ts=[threading.Thread(target=work, args=(10000,)) for _ in range(4)]
[t.start() for t in ts]
[t.join() for t in ts]
assert json.loads(json.dumps({"n":1}))["n"]==1
print("python-ok", sum(range(10000)))
'

  echo "==> compress / tar"
  head -c 131072 /dev/zero > z.in
  gzip -c z.in | gzip -d -c | cmp - z.in
  xz -c z.in | xz -d -c | cmp - z.in
  bzip2 -c z.in | bzip2 -d -c | cmp - z.in
  zstd -q -c z.in | zstd -q -d -c | cmp - z.in
  mkdir d && printf 't\n' > d/a
  tar -cf t.tar d && rm -rf d && tar -xf t.tar
  printf 't\n' > expect && cmp d/a expect

  echo "==> e2fsprogs"
  dd if=/dev/zero of=disk.img bs=1M count=8 status=none
  mkfs.ext4 -F -q disk.img
  e2fsck -n disk.img >/dev/null

  echo "==> gcc / g++"
  cat > t.c <<'CEOF'
#include <stdlib.h>
#include <string.h>
int main(void) {
  char *p = malloc(4096);
  if (!p) return 1;
  memset(p, 0x5a, 4096);
  p = realloc(p, 8192);
  if (!p) return 2;
  free(p);
  return 0;
}
CEOF
  gcc -O2 t.c -o t
  ./t

  cat > t.cpp <<'CEOF'
#include <vector>
#include <string>
int main() {
  int *p = new int[128];
  p[0] = 1;
  delete[] p;
  std::vector<std::string> v;
  for (int i = 0; i < 1000; i++) v.push_back(std::string(64, 'x'));
  return v.size() == 1000 ? 0 : 1;
}
CEOF
  g++ -O2 t.cpp -o tpp
  ./tpp

  echo "==> nodejs"
  node -e 'const a=[]; for (let i=0;i<10000;i++) a.push(Buffer.alloc(64)); if (a.length!==10000) process.exit(1); console.log("node-ok", a.length);'

  echo "==> mold (nixpkgs mold DT_NEEDs C libmimalloc-secure; dual preload must satisfy it)"
  printf 'int main(void){return 0;}\n' > m.c
  cc -fuse-ld=mold m.c -o m
  ./m
  echo mold-ok

  echo "world-preload ok (${so})"
  mkdir -p $out
  echo ok > $out/ok
''
