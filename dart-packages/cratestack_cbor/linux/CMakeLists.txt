# cratestack_cbor's Linux FFI-plugin build file (cratestack#563).
#
# Deliberately NOT the cargokit shape most flutter_rust_bridge plugins use
# (build Rust from source at consumer build time via an ExternalProject /
# CMake-invokes-cargo step) — the maintainer decision recorded on the ticket
# rejected imposing a Rust toolchain on every consuming Flutter developer's
# machine and CI. This file has no C/C++ sources and does not invoke cargo;
# its only job is to hand the ALREADY-VENDORED prebuilt library
# (`../blobs/linux-x64/libcratestack_client_flutter.so`, produced by `just
# cbor-vendor-native` — see this package's README) to Flutter's own
# bundled-library mechanism, the same one `flutter create -t plugin_ffi`
# scaffolds (see `<plugin>_bundled_libraries`, consumed by
# `flutter/generated_plugins.cmake` in every Flutter Linux app and installed
# into the built app's `bundle/lib/` next to the executable — verified by
# building a real app; see docs/tooling/cratestack-cbor-development.md).
cmake_minimum_required(VERSION 3.10)

set(PROJECT_NAME "cratestack_cbor")
project(${PROJECT_NAME} LANGUAGES CXX)

set(_cratestack_cbor_vendored_lib
  "${CMAKE_CURRENT_SOURCE_DIR}/../blobs/linux-x64/libcratestack_client_flutter.so"
)

if(NOT EXISTS "${_cratestack_cbor_vendored_lib}")
  message(FATAL_ERROR
    "cratestack_cbor: vendored native library not found at "
    "${_cratestack_cbor_vendored_lib}. This is expected on a fresh clone of "
    "the cratestack repo — the vendored artifact is build output and is not "
    "committed. Run `just cbor-vendor-native` first. If you installed this "
    "package from pub.dev, the archive should have shipped this file; the "
    "installation may be corrupt."
  )
endif()

# List of absolute paths to libraries that should be bundled with the
# plugin. Flutter's generated `flutter/generated_plugins.cmake` reads
# `${<plugin_name>_bundled_libraries}` (PARENT_SCOPE) from every FFI plugin
# and appends it to `PLUGIN_BUNDLED_LIBRARIES`, which the app's own
# `linux/CMakeLists.txt` installs into `${INSTALL_BUNDLE_LIB_DIR}`
# (`bundle/lib/`) — see the Flutter SDK's
# `templates/app/linux.tmpl/CMakeLists.txt.tmpl` for both halves of this
# contract. The app's `CMAKE_INSTALL_RPATH` is `$ORIGIN/lib`, which is what
# lets `lib/src/native/native_cbor_codec.dart` open this library by its
# bare filename at runtime with no path computation needed.
set(cratestack_cbor_bundled_libraries
  "${_cratestack_cbor_vendored_lib}"
  PARENT_SCOPE
)
