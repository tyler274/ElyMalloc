{
  mold-unwrapped,
  mimalloc,
}:

# Rebuild nixpkgs mold with this rewrite statically linked (whole-archive so
# the .init_array ctor is not dropped across rustc codegen units).
(mold-unwrapped.override { inherit mimalloc; }).overrideAttrs (old: {
  pname = "mold-unwrapped";
  postPatch =
    (old.postPatch or "")
    + ''
      substituteInPlace CMakeLists.txt \
        --replace-fail 'target_link_libraries(mold PRIVATE mimalloc)' \
                       'target_link_libraries(mold PRIVATE "-Wl,--whole-archive" mimalloc "-Wl,--no-whole-archive")'
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
      if ! nm "$out/bin/mold" | grep -q ' mi_malloc'; then
        echo "error: mi_malloc not found in mold (static mimalloc missing)" >&2
        exit 1
      fi
    '';
})
