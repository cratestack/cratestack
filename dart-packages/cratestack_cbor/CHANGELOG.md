## 0.8.6 (2026-08-21)

<!-- TODO: edit this section from the seed below -->
<!-- seeded from v0.8.5..HEAD at c5de7571f769d3c8345567ad4e378183c8be6e88 -->

This is an auto-generated seed. Please rewrite into narrative prose describing
the changes in this release, grouped by concern. Refer to existing entries in
this file for the house prose style. Do not commit with this placeholder text.

### Changes

#### Features

- thread If-Match through generated update/delete, surface ETag on reads (#610) (#671)
- support required-enum-field literal comparisons in read policies (#666) (#676)
- add cratestack_annotations + cratestack_builder packages (#668 phase 1) (#672)

#### Fixes

- seed and check changelogs from a declared list, not a hardcoded path (#669)
- encode serde_json::Value::Null as CBOR null on POST /rpc/batch (#657) (#675)
- re-fence invoke_with_db's illustrative doc example so `-- --ignored` can't force-compile it (#611) (#681)
- omit untouched update-input fields from the wire, every arity, both clients (#663) (#673)
- bring the two new Dart packages into version lockstep at 0.8.5 (#674)

#### Documentation

- consolidated 0.8.5 entries for today's merged batch (#680)

#### Chores

- resolve cratestack_builder against the published annotations package (#678)

#### CI

- publish cratestack_annotations + cratestack_builder on tag push (#682)

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
