## Unreleased

- **`flutter_rust_bridge` moves from 2.12.0 to 2.13.0.** This is the pin that
  decides which Flutter apps can depend on this package at all: a bare version
  is an exact pin in pub's grammar, so an app on any other flutter_rust_bridge
  version cannot add `cratestack_cbor` — `pub get` fails during version
  solving (cratestack#716). The pin cannot be widened (see below), so moving it
  is the only lever there is.

  **This is a breaking change for anyone currently on 2.12.0**, and a fix for
  anyone on 2.13.0. If you are pinned to a 2.13.0 *prerelease* such as
  `2.13.0-beta.6`, you are still blocked and need to move to stable 2.13.0 —
  pub excludes prereleases from ranges, so there is no constraint we can write
  that admits both.

  Verified end to end rather than by editing version strings: glue regenerated
  with codegen 2.13.0, `cargo build --features frb-glue`, the Dart round-trip
  harness, and this package's own `dart test` (7 tests) all pass, with the
  cross-binding CBOR fixtures still matching byte for byte — the wire format
  is unchanged by the upgrade.

- **The pin is now documented as an install-blocking constraint**, in a README
  section placed ahead of the quickstart rather than left implicit in a
  dependency line. It explains why a range is not an option: a range resolves
  to the *newest* match while the shipped glue is fixed at one version, so it
  would work today and start handing consumers 2.14.0 against 2.13.0 glue the
  day upstream publishes it — breaking on upstream's release schedule rather
  than ours, with our CI still green. The README also documents the workaround
  an affected app has today (`cratestack generate-dart --no-native-cbor`, the
  pure-Dart codec, which has no flutter_rust_bridge dependency) and notes that
  web-only apps are constrained by the pin too, since pub has no conditional
  dependencies and the web backend imports no flutter_rust_bridge at all.

  **Correction:** the first draft of these docs claimed flutter_rust_bridge's
  codegen "rejects a ranged constraint outright". That was wrong and is
  retracted. The `bail!("unexpected version range")` it cited applies to
  `ffigen`, and reaches `flutter_rust_bridge` only through an `.is_ok()` in
  `auto_upgrade.rs` that discards it. Measured: `just cbor-vendor-glue` runs to
  completion with a ranged constraint in `pubspec.yaml`. Tooling does not block
  a range — the runtime version mismatch does, and that alone is sufficient.

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
