# Imported targets for the Rust mimalloc rewrite.
# `mimalloc` is the static archive (mold's find_package(mimalloc 3) name).
# The shared libraries still live next to this file for LD_PRELOAD.

get_filename_component(_MIMALLOC_PREFIX "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)
set(_MIMALLOC_STATIC "${_MIMALLOC_PREFIX}/lib/libmimalloc-secure.a")
if(NOT EXISTS "${_MIMALLOC_STATIC}")
  set(_MIMALLOC_STATIC "${_MIMALLOC_PREFIX}/lib/libmimalloc.a")
endif()
if(NOT EXISTS "${_MIMALLOC_STATIC}")
  message(FATAL_ERROR "mimalloc static library not found under ${_MIMALLOC_PREFIX}/lib")
endif()

if(NOT TARGET mimalloc)
  add_library(mimalloc STATIC IMPORTED)
  set_target_properties(mimalloc PROPERTIES
    IMPORTED_LOCATION "${_MIMALLOC_STATIC}"
    INTERFACE_INCLUDE_DIRECTORIES "${_MIMALLOC_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "pthread;dl"
  )
endif()

if(NOT TARGET mimalloc-static)
  add_library(mimalloc-static STATIC IMPORTED)
  set_target_properties(mimalloc-static PROPERTIES
    IMPORTED_LOCATION "${_MIMALLOC_STATIC}"
    INTERFACE_INCLUDE_DIRECTORIES "${_MIMALLOC_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "pthread;dl"
  )
endif()

set(MIMALLOC_INCLUDE_DIR "${_MIMALLOC_PREFIX}/include")
set(MIMALLOC_LIBRARY_DIR "${_MIMALLOC_PREFIX}/lib")
unset(_MIMALLOC_PREFIX)
unset(_MIMALLOC_STATIC)
