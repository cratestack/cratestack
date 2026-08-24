## Unreleased

- **The `flutter_rust_bridge: 2.12.0` pin is now documented as an
  install-blocking constraint**, in a README section placed ahead of the
  quickstart rather than left implicit in a dependency line. A bare version is
  an exact pin in pub's grammar, so any app already depending on a different
  flutter_rust_bridge version cannot add this package at all — `pub get` fails
  during version solving. No behaviour change; the pin is unmoved.

  The pin cannot be relaxed from this end, and the docs now say why rather
  than leaving the next person to re-derive it: flutter_rust_bridge requires
  its codegen, Dart runtime, and Rust runtime to be exactly equal (stated
  upstream policy), enforces it with `==` on a `String` in
  `BaseEntrypoint.initImpl`, rejects a ranged constraint in its own codegen,
  and declined to make minor versions compatible
  (fzyzcjy/flutter_rust_bridge#2694). Widening the constraint would only move
  the failure from `pub get` to `createCborCodec()`, since the vendored native
  library is already compiled against 2.12.0's Rust runtime.

  The README now also documents the workaround an affected app actually has
  today — `cratestack generate-dart --no-native-cbor`, the pure-Dart codec,
  which has no flutter_rust_bridge dependency — and notes that web-only apps
  are constrained by the pin too, since pub has no conditional dependencies
  and the web backend imports no flutter_rust_bridge at all.

- **Linux arm64 is now documented as blocked upstream rather than as pending
  work.** Flutter publishes no arm64 Linux SDK on any channel (verified
  against the release manifest: 732 entries, all x64, zero containing `arm`
  or `aarch`), and a spike on a real `ubuntu-24.04-arm` runner confirmed
  `flutter build linux` therefore cannot run on such a host. No behaviour
  change — the platform already threw a clear `UnsupportedError`; the message
  and the docs now say *why*, and distinguish the Flutter case (impossible)
  from plain `dart test`/`dart run` on arm64 Linux (still open — the Dart
  SDK, unlike Flutter's, does ship for that host).

## 0.8.10 (2026-08-23)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

## 0.8.9 (2026-08-23)

- No functional changes. Version kept in lockstep with the CrateStack
  workspace, which every published CrateStack artifact shares.

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
