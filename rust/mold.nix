{
  mold-unwrapped,
  mimalloc,
}:

# Rebuild nixpkgs mold with this rewrite statically linked.
#
# CMake 3.24+ `$<LINK_LIBRARY:WHOLE_ARCHIVE,…>` is required: splitting
# `-Wl,--whole-archive` / `mimalloc` / `-Wl,--no-whole-archive` lets CMake
# reorder the flags and drop the archive (mold's TUs do not call `mi_*`
# after we disable `mimalloc-new-delete.h`). Whole-archive also keeps the
# `.init_array` ctor across rustc codegen units.
#
# `MOLD_USE_SYSTEM_MIMALLOC` makes mold `#include <mimalloc-new-delete.h>`
# in entry.cc. That is for a *dynamic* libmimalloc.so. C mimalloc (and this
# rewrite) already export strong Itanium new/delete from the static archive,
# so including the header as well is a multiple-definition error.
(mold-unwrapped.override { inherit mimalloc; }).overrideAttrs (old: {
  pname = "mold-unwrapped";
  postPatch =
    (old.postPatch or "")
    + ''
      substituteInPlace CMakeLists.txt \
        --replace-fail 'target_link_libraries(mold PRIVATE mimalloc)' \
                       'target_link_libraries(mold PRIVATE "$<LINK_LIBRARY:WHOLE_ARCHIVE,mimalloc>")'
      cat >> CMakeLists.txt <<'CMAKE'

# Pull `mi_malloc` even if WHOLE_ARCHIVE is ignored, and keep it in .dynsym
# so `nm -D` / installCheck can see the hidden-visibility Rust C ABI.
target_link_options(mold PRIVATE
  "LINKER:--undefined=mi_malloc"
  "LINKER:--export-dynamic-symbol=mi_malloc"
)
CMAKE
      substituteInPlace src/entry.cc \
        --replace-fail '#if MOLD_USE_SYSTEM_MIMALLOC' \
                       '#if 0 // static mimalloc already exports new/delete'
    '';
  postInstallCheck =
    (old.postInstallCheck or "")
    + ''
      echo "==> mold must statically contain Rust mimalloc (no DT_NEEDED)"
      readelf -d "$out/bin/mold"
      if readelf -d "$out/bin/mold" | grep NEEDED | grep -q libmimalloc; then
        echo "error: mold DT_NEEDED libmimalloc; expected a static archive" >&2
        exit 1
      fi
      # Don't pipe `readelf` into `grep -q`: grep exits on the first match,
      # readelf gets SIGPIPE, and with `pipefail` the pipeline is 141 - which
      # `if !` treats as failure even when `mi_malloc` is present.
      readelf -s --wide "$out/bin/mold" > "$TMPDIR/mold.syms"
      if ! grep -F -w -- mi_malloc "$TMPDIR/mold.syms" >/dev/null; then
        echo "error: mi_malloc not found in mold (static mimalloc missing)" >&2
        grep -F -e mi_malloc -e _Znwm -e ' malloc' "$TMPDIR/mold.syms" >&2 || true
        exit 1
      fi
      if ! grep -F -w -- _Znwm "$TMPDIR/mold.syms" >/dev/null; then
        echo "error: operator new not found in mold (C++ ABI stripped)" >&2
        exit 1
      fi
    '';
})
