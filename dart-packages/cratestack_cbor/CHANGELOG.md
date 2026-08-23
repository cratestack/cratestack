## Unreleased

## 0.8.7 (2026-08-23)

- **Adds macOS, Windows and iOS.** The package previously supported Linux x64,
  web and Android; it now also ships prebuilt binaries for **macOS**
  (arm64 + x86_64, one universal xcframework), **Windows** x64, and **iOS**
  (device `ios-arm64` plus a universal simulator slice). As with every other
  platform here, these are vendored prebuilt artifacts: no Rust toolchain, no
  cargokit, and no network fetch at your build time. The same CBOR fixture
  round-trips byte-identically on all six targets, each verified by building
  and running a real Flutter app rather than by compiling alone.
- **Linux arm64 remains unsupported**, and is the only platform left in the
  matrix. Every other platform the package claims now has a real prebuilt
  binary and a real end-to-end test behind it.
- The macOS xcframework is shipped as a `.zip` inside the archive and unpacked
  by the plugin's CocoaPods `prepare_command` at pod-install time. This is
  invisible if you just depend on the package, and is required because
  `dart pub publish` dereferences symlinks: a macOS framework is a versioned
  bundle whose symlinks are structural, and without them `codesign` rejects it
  and `flutter build macos` fails. iOS frameworks are shallow bundles with no
  symlinks, so iOS ships unpacked.
- The archive grew accordingly — every consumer carries every platform's
  payload, which is the cost of one package covering the whole matrix.

## 0.8.6 (2026-08-21)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.3

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.
- First version published by the automated release pipeline rather than by
  hand (cratestack#563).

## 0.8.2

Package metadata only — the codec, the platform support and the vendored
artifacts are unchanged from 0.8.0.

- Declares `environment.flutter: ">=1.20.0"`. This package deliberately ships
  no `ios/` folder, and Flutter only permits a plugin to omit platform folders
  from 1.20 onward — pub.dev rejects the upload otherwise. The Dart constraint
  (`sdk: ^3.5.0`) remains the real floor.
- Removes the `publish_to: none` guard that kept earlier revisions from being
  published by accident.
- Shortens the package description to pub.dev's 180-character recommendation.

(0.8.1 was never published to pub.dev.)

## 0.8.0

- Initial package structure (cratestack#563). One uniform `CratestackCborCodec`
  API, auto-selected per platform:
  - Native: flutter_rust_bridge over a vendored prebuilt library. This
    release vendors **Linux x86_64 and Android (arm64-v8a, x86_64,
    armeabi-v7a)** — the remaining platform matrix (iOS, macOS, Windows,
    Linux arm64) is follow-up work.
  - Web: the existing `cratestack-cbor-wasm` wasm-bindgen artifact,
    vendored and loaded via `dart:js_interop`.
- Flutter app integration, proven by real builds (cratestack#563):
  - Linux: a Flutter FFI plugin (`linux/CMakeLists.txt`) bundles the
    vendored `.so` into a real `flutter build linux` app, instead of the
    `cargokit` build-Rust-from-source pattern most flutter_rust_bridge
    plugins use.
  - Android: a Flutter FFI plugin (`android/build.gradle`) packages the
    vendored per-ABI `.so` files into the APK via the standard `jniLibs`
    mechanism — no CMake/NDK invocation at consumer build time. Verified
    by a real `flutter build apk`, per-ABI presence assertion, and a
    real install-and-run on an Android emulator round-tripping CBOR.
  - Web: `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm`
    pair so a release `flutter build web` actually ships them; the web
    loader now tries both the dev-server and release asset URL
    conventions.
  - `example/`: a minimal Flutter app exercising the codec, verified with
    real `flutter build linux`/`flutter build web`/`flutter build apk`
    builds — see `just cbor-example-verify` and
    `just cbor-example-verify-android`.
