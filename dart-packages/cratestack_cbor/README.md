# cratestack_cbor

Native CBOR codec for CrateStack Dart/Flutter clients (cratestack#563). One
uniform API, two backends selected automatically per platform — mirrors
[`@cratestack/cbor`](../../packages/cratestack-cbor)'s conditional-export
umbrella shape for JavaScript (`@cratestack/cbor-node` / `@cratestack/cbor-web`
behind one import path).

```dart
import 'package:cratestack_cbor/cratestack_cbor.dart';

final codec = await createCborCodec();
final bytes = codec.encodeJson('{"hello":"world"}');
final json = codec.decodeJson(bytes);
```

## Backends

| Platform | Backend | Artifact |
| --- | --- | --- |
| Native (`dart.library.io`) | [flutter_rust_bridge](https://pub.dev/packages/flutter_rust_bridge) `=2.12.0` over `crates/cratestack-client-flutter`'s `cbor` module | A **vendored prebuilt native library** — `blobs/linux-x64/libcratestack_client_flutter.so` (Linux), `blobs/android/<abi>/libcratestack_client_flutter.so` (Android: arm64-v8a, x86_64, armeabi-v7a), `blobs/windows-x64/cratestack_client_flutter.dll` (Windows), or `macos/Frameworks/CratestackCborNative.xcframework` (macOS, universal arm64 + x86_64) in this release. No Rust toolchain, no network fetch, at consumer build time. |
| Web (`dart.library.js_interop`) | The **existing** [`cratestack-cbor-wasm`](../../crates/cratestack-cbor-wasm) wasm-bindgen artifact (already shipped to npm as [`@cratestack/cbor-web`](../../packages/cratestack-cbor-web)) | A **vendored** `wasm-pack --target web` build — `lib/src/web/wasm-pkg/` — loaded at runtime via `dart:js_interop`. No new codec binding; this reuses the exact same Rust wasm-bindgen crate the JS package already binds. |

Linux, Android, Windows, and macOS share the same flutter_rust_bridge Dart glue (`lib/src/native/rust/`, platform-independent — frb codegen introspects Rust source, not a target triple) but vendor three genuinely different resolution mechanisms: Linux and Windows each resolve the vendored library by an executable-relative path (the two paths differ — no `lib/` subdirectory on Windows, see `windows/CMakeLists.txt`), Android resolves it by bare SONAME from the app's own native library directory (populated by `android/build.gradle`'s `jniLibs.srcDirs`), and macOS resolves it by a *fixed* relative framework path with no computation at all — that only works because CocoaPods **links** the vendored xcframework into the built app rather than merely copying it (see `macos/cratestack_cbor.podspec` and `lib/src/native/native_cbor_codec.dart`).

Both backends round-trip byte-identical CBOR to `cratestack-codec-cbor`'s
`CborCodec` for the same input — see `test/shared_fixtures.dart`, which both
`test/native_cbor_codec_test.dart` and `test/web_cbor_codec_test.dart` assert
against, using the **same** hex fixtures `cratestack-cbor-napi`,
`cratestack-cbor-wasm`, and `crates/cratestack-client-flutter`'s own test
suites assert (three independent bindings agreeing on the same wire bytes).

## Why a JSON-text boundary, not a native Dart value type

flutter_rust_bridge has no dynamic "any JSON value" wire type the way napi's
`serde-json` feature or wasm-bindgen's `JsValue` do — every bridged Rust
function signature is a concrete, statically-known type its codegen can walk
ahead of time. So `encodeJson`/`decodeJson` cross the native FFI boundary as
`String` (JSON text), and the web backend normalizes to the exact same
boundary (via the browser's own `JSON.parse`/`JSON.stringify`) so **callers
never branch on platform** — one uniform `CratestackCborCodec` interface, see
`lib/src/cbor_codec.dart`.

## Scope of this release — read before depending on this in production

This is a **partial platform matrix** (cratestack#563), not the full package:

- **Native platform matrix:** Linux x86_64, Android (arm64-v8a, x86_64,
  armeabi-v7a), Windows x86_64, and macOS (arm64 + x86_64, universal) only.
  `resolveVendoredLibraryPath()` throws a clear `UnsupportedError` on every
  other platform (iOS, Linux arm64) rather than silently failing. The
  remaining matrix is deliberate follow-up work, not an oversight.
- **Not published to pub.dev yet.** The publish workflow (GitHub Actions
  OIDC, verified publisher `cratestack.dev`) and version-locking to the
  workspace version both exist — see
  `docs/tooling/dart-publishing.md` — but pub.dev cannot automate a brand
  new package's *first* publish (only subsequent versions). Until a
  maintainer performs that one-time manual `dart pub publish` and enables
  Automated publishing on pub.dev's Admin tab, this package stays
  unpublished and `publish-pubdev-cbor` in `release-cli.yml` fails on every
  tag push by design, rather than silently skipping.
- **The Dart generator does not use this package yet.**
  `crates/cratestack-client-dart/templates/pubspec.yaml.j2` still emits
  `cbor: ^6.5.1` — flipping that seam before this package is published would
  break `dart pub get` for every generated client (pub.dev returns 404 for
  an unpublished package name).

## Flutter app integration — proven, not just `dart test`

`dart test`/`dart test -p chrome` prove the codec works as a Dart *library*.
They do not prove it works inside a real Flutter *app*: a compiled app has no
package source tree for `Isolate.resolvePackageUri` to find, a release web
bundle has no dev server to serve the `packages/...` URL convention from, and
Android/macOS have no `dart test` story at all. This package closes that gap
for **Linux desktop, Android, Windows desktop, macOS desktop, and web**:

- **Linux desktop:** `linux/CMakeLists.txt` makes this package a Flutter FFI
  plugin (`pubspec.yaml`'s `flutter: plugin: platforms: linux: ffiPlugin:
  true`) that hands the vendored `blobs/linux-x64/libcratestack_client_flutter.so`
  to Flutter's own `<plugin>_bundled_libraries` mechanism — the same one
  `flutter create -t plugin_ffi` scaffolds, **not** the `cargokit` pattern
  most flutter_rust_bridge plugins use (that builds Rust from source at
  consumer build time; the maintainer decision on cratestack#563 rejected
  imposing a Rust toolchain on every consuming Flutter developer). A real
  `flutter build linux` copies the library into the built bundle's `lib/`,
  next to the app executable; `lib/src/native/native_cbor_codec.dart`
  resolves it there first (falling back to the dev-mode
  `Isolate.resolvePackageUri` path only for `dart test`/`dart run`).
  Deliberately **no** `flutter: sdk: flutter` dependency on this package —
  verified empirically that Flutter's plugin tooling honors the `flutter:`
  pubspec key without it, so plain `dart pub get`/`dart test` (no Flutter SDK
  at all) keep working exactly as before.
- **Android:** `android/build.gradle` makes this package an Android FFI
  plugin (`pubspec.yaml`'s `flutter: plugin: platforms: android: ffiPlugin:
  true`) that packages the vendored per-ABI libraries
  (`blobs/android/<abi>/libcratestack_client_flutter.so`) into the APK via
  the standard `jniLibs.srcDirs` source-set mechanism — again, no
  cargokit/CMake/NDK invocation at consumer build time, same constraint as
  Linux. This is a genuinely different resolution mechanism from Linux, not
  a relocated copy of it: Android's dynamic linker finds a bundled library
  by bare SONAME from the app's own native library directory, so
  `native_cbor_codec.dart`'s Android branch computes no path at all — it
  just opens `libcratestack_client_flutter.so` directly. See
  `docs/tooling/cratestack-cbor-development.md`'s gotcha 6 for why an APK
  that *compiles* does not by itself prove the library shipped.
- **Windows desktop:** `windows/CMakeLists.txt` makes this package a Flutter
  FFI plugin (`pubspec.yaml`'s `flutter: plugin: platforms: windows:
  ffiPlugin: true`) that hands the vendored
  `blobs/windows-x64/cratestack_client_flutter.dll` to the SAME
  `<plugin>_bundled_libraries` mechanism Linux uses — one shared Flutter-SDK
  template renders both platforms' `generated_plugins.cmake` glue. The
  install destination differs, though: Flutter's own Windows app template
  copies bundled libraries directly next to the built `.exe`, not into a
  `lib/` subdirectory (see `windows/CMakeLists.txt`'s header comment), so
  `native_cbor_codec.dart`'s Windows branch looks there instead.
  **Unverified by this repo's own CI/toolchain** (Linux-only —
  cross-compiling `x86_64-pc-windows-msvc` and running `flutter build
  windows` both require a Windows host); `just cbor-example-verify-windows`
  is the CI-facing proof, and `cratestack-cbor-windows` in
  `.github/workflows/ci.yml` is its first real execution.
- **macOS desktop:** `macos/cratestack_cbor.podspec` makes this package a
  Flutter FFI plugin (`pubspec.yaml`'s `flutter: plugin: platforms: macos:
  ffiPlugin: true`) that hands the vendored universal xcframework
  (`macos/Frameworks/CratestackCborNative.xcframework`, produced by `just
  cbor-vendor-macos`) to **CocoaPods'** `vendored_frameworks` — a genuinely
  different mechanism from the other three platforms, not a relocated copy:
  CocoaPods (Flutter's default macOS build path; verified NOT Swift Package
  Manager for a plugin like this one with no `Package.swift` — see the
  podspec's own header comment) **links** the vendored framework into the
  built app rather than merely copying it, so `native_cbor_codec.dart`'s
  macOS branch resolves it with a *fixed* relative string and computes no
  path at all — closer in spirit to Android's "no path to compute" than to
  Linux/Windows' executable-relative fallback chain, for a different
  underlying reason (dyld matching an already-linked image, not SONAME
  resolution). No dev-mode fallback exists either, so — like Android — the
  native backend has no `dart test` story on macOS; `example/` and `just
  cbor-example-verify-macos` are the only way to exercise it. **Unverified
  by this repo's own CI/toolchain** in the same sense Windows was before its
  own first CI run (Linux-only dev machine, no Xcode/`lipo`/`xcodebuild`);
  `just cbor-vendor-macos`'s exact assembly sequence and this mechanism were
  proven on a real `macos-latest` runner by a throwaway spike branch
  (`spike/cbor-macos-xcframework`) before landing here, and
  `cratestack-cbor-macos` in `.github/workflows/ci.yml` is this recipe's
  first real execution wired into CI.
- **Web:** `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm` pair
  as real Flutter assets, so a release `flutter build web` copies them into
  `build/web/assets/packages/cratestack_cbor/...`. `web_cbor_codec.dart` tries
  the dev-server `packages/...` URL first (unchanged, still what `dart test
  -p chrome`/`flutter run -d chrome` use), then falls back to the release
  `assets/packages/.../lib/...` URL — the two conventions coexist, neither
  subsumes the other.

`dart-packages/cratestack_cbor/example/` is a minimal Flutter app proving
all five, with real builds: see that directory's README, `just
cbor-example-verify` (Linux+web, wired into CI as `cratestack-cbor-example`),
`just cbor-example-verify-android` (Android APK build + per-ABI presence
proof, wired into CI as `cratestack-cbor-android`), `just
cbor-example-verify-windows` (Windows `.exe` build + DLL presence proof,
wired into CI as `cratestack-cbor-windows`), and `just
cbor-example-verify-macos` (macOS `.app` build + universal-xcframework
presence proof, wired into CI as `cratestack-cbor-macos`). Linux, web,
Windows, and macOS all build in **release** mode and actually run the built
app — Linux headless via `xvfb-run`, web served and driven by a real
headless Chrome, Windows and macOS run directly (hosted runners have a real
desktop session, no headless wrapper needed) — not `flutter run`'s dev
server, which resolves assets differently and would prove nothing about a
release deploy. Android additionally has a **local/manual** companion, `just
cbor-example-verify-android-emulator`, that installs the built APK on a real
Android emulator and asserts the app actually round-trips CBOR at runtime —
deliberately not wired into CI, since booting an emulator on a hosted runner
is substantially heavier and flakier than everything else this package's CI
already does; see that recipe's own comment in the `justfile` for the full
reasoning.

iOS and Linux arm64 remain out of scope for this slice (deliberately — see
the platform matrix note above).

## Regenerating the vendored artifacts

> **Working on this package rather than using it?** Read
> [docs/tooling/cratestack-cbor-development.md](https://github.com/cratestack/cratestack/blob/main/docs/tooling/cratestack-cbor-development.md)
> first. It covers the toolchain pins, the first-run steps, and four
> failure modes that each *look like success* — a missing `.pubignore`
> silently publishing a package without its binaries, a web test that
> passes against the native backend, a bridged function that is 2x slower
> than pure Dart because it is async, and rustfmt reformatting generated
> glue.

All artifacts are build outputs from crates in this repo and are
regenerated, not hand-written. From the repository root:

```bash
just cbor-vendor-native          # flutter_rust_bridge glue + blobs/linux-x64/*.so
just cbor-vendor-web             # wasm-pack --target web build -> lib/src/web/wasm-pkg/
just cbor-vendor-android         # cargo ndk cross-compile -> blobs/android/<abi>/*.so (reuses cbor-vendor-native's glue — run that first)
just cbor-vendor-lib windows-x64 # release build (must run ON Windows) -> blobs/windows-x64/*.dll (reuses cbor-vendor-native's glue — run that first)
just cbor-vendor-macos           # release build for BOTH Darwin arches (must run ON macOS) -> macos/Frameworks/CratestackCborNative.xcframework (reuses cbor-vendor-native's glue — run cbor-vendor-glue first)
```

See the `justfile` for what each does.

**None of the vendored output is `git`-tracked** — not the Dart glue at
`lib/src/native/rust/`, the native libraries at `blobs/linux-x64/`,
`blobs/android/<abi>/`, and `blobs/windows-x64/`, the assembled xcframework
at `macos/Frameworks/`, nor the wasm build at `lib/src/web/wasm-pkg/`. This
matches `CLAUDE.md`'s "don't
commit generated build output", cratestack#563's own "frb glue is generated
in CI, not committed" decision (which gitignores the byte-identical
`frb_generated.*` files in `crates/cratestack-client-flutter`), and the two
sibling npm packages: `@cratestack/cbor-node`'s compiled `.node` addon and
`@cratestack/cbor-web`'s `wasm-pkg/` are both gitignored, with the release
workflow building them immediately before publishing.

There is a real trap here, and `.pubignore` is what disarms it. Per
[pub.dev's publishing docs](https://dart.dev/tools/pub/publishing), `pub`
uses `git ls-files` to decide what to publish when the package directory
sits inside a Git working tree — so files excluded by `.gitignore` are
**silently omitted from the published tarball**, with no `files`-field
equivalent to opt them back in and no warning. Gitignoring these
directories naively would ship a package missing the very artifacts this
ticket exists to vendor.

`.pubignore` resolves it: when present in a directory, `pub` consults it
*instead of* `.gitignore` there. This package's `.pubignore` is
intentionally empty of exclusions, so the gitignored build outputs are
still packed. **Verified, not assumed** — `dart pub publish --dry-run` with
all four vendored artifact directories gitignored and untracked still lists
them all:

```
├── blobs
│   ├── android
│   │   ├── arm64-v8a
│   │   │   └── libcratestack_client_flutter.so (904 KB)
│   │   ├── armeabi-v7a
│   │   │   └── libcratestack_client_flutter.so (492 KB)
│   │   └── x86_64
│   │       └── libcratestack_client_flutter.so (946 KB)
│   └── linux-x64
│       └── libcratestack_client_flutter.so (903 KB)
...
│           ├── cratestack_cbor_wasm.js (21 KB)
│           └── cratestack_cbor_wasm_bg.wasm (121 KB)
```

Total compressed archive size is ~1 MB with all four native libraries plus
the wasm pair vendored — still a small fraction of pub.dev's 100 MB gzip
recommendation (see cratestack#563's thread for the original size-budget
analysis) — and `git ls-files` reports zero of them tracked. So the
published package vendors its binaries exactly as decided, while the
repository stays free of build output.

Regenerate them with the `just` recipes above; treat them as build outputs
to keep in sync with the source crates, never as source to hand-edit.
**`.pubignore` must stay tracked** — without it in a fresh
checkout, publishing silently reverts to dropping these files.

## Verifying both backends

```bash
just cbor-verify-package        # both backends, native (dart test) + web (dart test -p chrome)
```

or directly, from this directory:

```bash
dart test                        # native backend only (@TestOn('vm'))
dart test -p chrome              # web backend only (@TestOn('browser'))
```

`@TestOn('vm')` / `@TestOn('browser')` are load-bearing, not decorative — see
`test/web_cbor_codec_test.dart`'s doc comment for why: the conditional export
in `lib/cratestack_cbor.dart` is resolved by the *compile target*, not by
which test file imported it, so an unguarded `dart test` would otherwise
silently compile the "web" test file against the native backend on the VM
and report false-positive passes without ever touching `dart:js_interop`.

## Why `dart-packages/`, not `packages/`

The npm umbrella this mirrors lives at `packages/cratestack-cbor`
(`@cratestack/cbor`) — but `pnpm-workspace.yaml` globs `packages/*`, and
every existing entry under `packages/` is a pnpm workspace member (has a
`package.json`). This package has none (it's pure Dart/pub, not npm), so
placing it at `packages/cratestack_cbor` would put an unrecognized directory
inside pnpm's workspace glob. It also can't reuse the exact name
`packages/cratestack-cbor` (already the npm umbrella) even if collisions
were fine, since Dart requires a `snake_case` package name
(`cratestack_cbor`) while this repo's `packages/` entries are `kebab-case`.
`dart-packages/` is a new top-level directory, parallel to `crates/`
(Rust), `packages/` (npm), and `examples/` — reserved for Dart/pub packages,
not swept into any existing tool's workspace glob.

## License

MIT
