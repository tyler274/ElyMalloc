# LD_PRELOAD smoke of typical NixOS world packages against this rewrite.
# Not a substitute for Firefox/Chromium/Electron: those are
# rust/browsers.nix and `mimalloc-harness browsers`. Compile/link success
# is not enough: each binary must run.
{
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
  runCommand,
}:

let
  so = "${mimalloc}/lib/libmimalloc.so";
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
    xz
    bzip2
    git
    curl
    wget
    openssl
    python3
    perl
    ripgrep
    jq
    gnumake
    gcc
    binutils
    bash
  ];
} ''
  set -euo pipefail
  test -f ${so}
  export LD_PRELOAD=${so}
  export HOME=$TMPDIR
  export GIT_CONFIG_NOSYSTEM=1

  hello
  ls --version >/dev/null
  grep --version >/dev/null
  awk --version >/dev/null
  sed --version >/dev/null
  find --version >/dev/null
  diff --version >/dev/null
  tar --version >/dev/null
  gzip --version >/dev/null
  xz --version >/dev/null
  bzip2 --version >/dev/null
  git --version
  curl --version >/dev/null
  wget --version >/dev/null
  openssl version >/dev/null
  perl -e 'my $x = "a" x 10000; print length($x), "\n"'
  rg --version >/dev/null
  jq -n '{ok:true}' >/dev/null
  make --version >/dev/null
  gcc --version >/dev/null

  python3 -c 'import hashlib,threading; blob=bytes(i%256 for i in range(8000)); hashlib.sha256(blob*64).hexdigest(); print("python-ok", sum(range(10000)))'

  cat > t.c <<'EOF'
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
  EOF
  gcc -O2 t.c -o t
  ./t

  cat > t.cpp <<'EOF'
  int main() {
    int *p = new int[128];
    p[0] = 1;
    delete[] p;
    return 0;
  }
  EOF
  g++ -O2 t.cpp -o tpp
  ./tpp

  echo "world-preload ok (${so})"
  mkdir -p $out
  echo ok > $out/ok
''
