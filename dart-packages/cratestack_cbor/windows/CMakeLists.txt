# cratestack_cbor's Windows FFI-plugin build file (cratestack#563).
#
# Deliberately NOT the cargokit shape most flutter_rust_bridge plugins use
# (build Rust from source at consumer build time via an ExternalProject /
# CMake-invokes-cargo step) — the maintainer decision recorded on the ticket
# rejected imposing a Rust toolchain on every consuming Flutter developer's
# machine and CI, same as linux/CMakeLists.txt and android/build.gradle.
# This file has no C/C++ sources and does not invoke cargo; its only job is
# to hand the ALREADY-VENDORED prebuilt library
# (`../blobs/windows-x64/cratestack_client_flutter.dll`, produced by `just
# cbor-vendor-lib windows-x64` — see this package's README) to Flutter's own
# bundled-library mechanism, the same `<plugin>_bundled_libraries`
# (PARENT_SCOPE) → `PLUGIN_BUNDLED_LIBRARIES` contract Linux uses (one shared
# template, `_pluginCmakefileTemplate` in the Flutter SDK's
# `packages/flutter_tools/lib/src/flutter_plugins.dart`, rendered for both
# platforms) — verified by reading that file, not by building (this repo's
# toolchain is Linux-only; see docs/tooling/cratestack-cbor-development.md).
#
# The INSTALL DESTINATION differs from Linux, though: Flutter's own
# `templates/app/windows.tmpl/CMakeLists.txt.tmpl` sets
# `INSTALL_BUNDLE_LIB_DIR` to `${CMAKE_INSTALL_PREFIX}` — NO `/lib` suffix,
# unlike Linux's `${CMAKE_INSTALL_PREFIX}/lib` — because Windows copies
# support files directly next to the executable instead of into a separate
# bundle directory (so that running from inside Visual Studio also works;
# see that template's own "=== Installation ===" comment). So the vendored
# `.dll` lands beside the built `.exe`
# (`build/windows/x64/runner/<Debug|Profile|Release>/`), not inside a `lib/`
# subdirectory — see `../lib/src/native/native_cbor_codec.dart`'s Windows
# branch, which looks there rather than at Linux's `lib/` path.
#
# cmake_minimum_required is 3.14 here, not Linux's 3.10: this matches what
# Flutter's own `templates/plugin_ffi/windows.tmpl/CMakeLists.txt.tmpl`
# scaffolds — "developers have a version of Visual Studio installed that
# includes CMake 3.14 or later. You should not increase this version, as
# doing so will cause the plugin to fail to compile for some customers of
# the plugin" (that template's own comment, kept verbatim in spirit here).
cmake_minimum_required(VERSION 3.14)

set(PROJECT_NAME "cratestack_cbor")
project(${PROJECT_NAME} LANGUAGES CXX)

set(_cratestack_cbor_vendored_lib
  "${CMAKE_CURRENT_SOURCE_DIR}/../blobs/windows-x64/cratestack_client_flutter.dll"
)

if(NOT EXISTS "${_cratestack_cbor_vendored_lib}")
  message(FATAL_ERROR
    "cratestack_cbor: vendored native library not found at "
    "${_cratestack_cbor_vendored_lib}. This is expected on a fresh clone of "
    "the cratestack repo — the vendored artifact is build output and is not "
    "committed. Run `just cbor-vendor-lib windows-x64` first. If you "
    "installed this package from pub.dev, the archive should have shipped "
    "this file; the installation may be corrupt."
  )
endif()

# List of absolute paths to libraries that should be bundled with the
# plugin. Flutter's generated `flutter/generated_plugins.cmake` reads
# `${<plugin_name>_bundled_libraries}` (PARENT_SCOPE) from every FFI plugin
# and appends it to `PLUGIN_BUNDLED_LIBRARIES`, which the app's own
# `windows/CMakeLists.txt` installs into `${INSTALL_BUNDLE_LIB_DIR}` — see
# the file header above for why that resolves to right next to the built
# `.exe` on Windows, unlike Linux's `bundle/lib/`.
set(cratestack_cbor_bundled_libraries
  "${_cratestack_cbor_vendored_lib}"
  PARENT_SCOPE
)
