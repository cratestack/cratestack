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

`createCborCodec()` is **idempotent** — call it from as many places as you like,
concurrently or not, and you get the same one-time initialization back
(cratestack#794). That matters because you rarely have only one caller: an app
that uses this package directly *and* has a generated `transport rpc` Dart
client has two, and the generated client calls it from inside its own runtime
where you cannot hand it a codec you already built. It also respects a runtime
you bootstrapped yourself, rather than trying to initialize
`flutter_rust_bridge` a second time. `isCborRuntimeInitialized` (exported
alongside `createCborCodec` on every platform) reports whether the backend
runtime is already up, for a consumer with its own bootstrap path that wants to
cooperate rather than guess.

## The `flutter_rust_bridge` pin — read this before `pub add`

This package depends on **`flutter_rust_bridge: 2.13.0`**, and in pub's
grammar a bare version is an *exact* pin, not a range. The practical
consequence is blunt:

> **If your app (or any other package in your dependency graph) already
> depends on a different `flutter_rust_bridge` version, you cannot add
> `cratestack_cbor` at all.** `pub get` fails during version solving,
> before any code runs.

```
Because your_app depends on flutter_rust_bridge 2.12.0 and every version of
cratestack_cbor depends on flutter_rust_bridge 2.13.0, cratestack_cbor is
forbidden.
```

**This is not a constraint CrateStack chose, and it is not one CrateStack can
relax.** flutter_rust_bridge requires that its codegen, its Dart runtime
package, and its Rust runtime crate all be *exactly* the same version — see
[upstream's compatibility page](https://cjycode.com/flutter_rust_bridge/guides/miscellaneous/compatibility),
which states that all flutter_rust_bridge packages "will need to have exactly
the same version". It is enforced in the generated code, not merely
recommended: the glue this package ships carries `String get codegenVersion =>
'2.13.0'`, and flutter_rust_bridge's `BaseEntrypoint.initImpl` compares that
against its own runtime constant with `==` on a plain `String` and throws a
`StateError` on any difference. flutter_rust_bridge's own project scaffolding
emits the same exact pins it demands of us (`flutter_rust_bridge = "=X.Y.Z"`
in `Cargo.toml`, bare `flutter_rust_bridge: X.Y.Z` in `pubspec.yaml`). Upstream
[issue #2694](https://github.com/fzyzcjy/flutter_rust_bridge/issues/2694)
asked for cross-minor-version compatibility for exactly this reason and was
closed without it.

**Why not a range like `^2.13.0` or `>2.12.0`?** Because a range resolves to
the *newest* matching version, and this package's glue is fixed at one. Today
2.13.0 is newest, so a range would appear to work. The day flutter_rust_bridge
publishes 2.14.0, every fresh `pub get` starts selecting it while the glue in
this archive still declares `2.13.0` — and every new consumer gets a
`StateError` on first use. Nothing in this repository would have changed, and
our CI would stay green, because the breakage is triggered by upstream's
release schedule rather than by anything we do. An exact pin fails loudly at
install time, in a way we control and can fix; a range fails quietly at
runtime, at a moment chosen by someone else.

A range also does not do what people usually hope. Pub excludes prereleases
from ranges, so if your app is pinned to something like `2.13.0-beta.6`, no
range on our side admits it — `>2.12.0 <2.13.0` matches *no versions at all*.
Converging on the same stable release is the only thing that actually works.

**`dependency_overrides` is not a workaround — it is a worse failure.** It
gets you past `pub get`, and then the version check above throws at
`createCborCodec()` instead. You have converted an install-time error into a
runtime one. Upstream's own bypass for that check
(`forceSameCodegenVersion: false`) is not reachable either — it is a parameter
on the generated entrypoint's `init`, which this package calls internally
(`lib/src/native/native_cbor_codec.dart`) and does not expose. And even if it
were, the vendored native library here was *compiled* against
flutter_rust_bridge 2.13.0's Rust runtime; nothing on the Dart side changes
that half of the pair.

**What to do instead**, in the order you should consider them:

1. **Converge on 2.13.0** if the other package in your graph can move. The
   mechanical part is cheaper than it looks — install
   `flutter_rust_bridge_codegen` 2.13.0 and re-run `generate`, which rewrites
   the Dart *and* Rust dependency for you. The cost is the re-validation:
   this regenerates that package's bridge glue, so budget for re-testing its
   native surface, not just for the version edit.
2. **Use the pure-Dart CBOR codec**, which has no flutter_rust_bridge
   dependency at all: pass `--no-native-cbor` to `cratestack generate-dart`.
   You give up the native codec's throughput, not correctness — the pure-Dart
   path round-trips the same wire bytes.
3. **Tell us which version you need.** The pin is a maintainer decision that
   tracks one flutter_rust_bridge release at a time; if adoption is
   concentrating on a newer one, that is worth knowing. Open an issue on
   [the tracker](https://github.com/cratestack/cratestack/issues).

One wrinkle worth naming, because it surprises people: **web-only apps pay
this pin too.** The web backend is wasm-bindgen and imports no
flutter_rust_bridge whatsoever, but pub has no conditional-dependency
mechanism, so the pin sits in `pubspec.yaml` unconditionally and constrains
every consumer regardless of which backend they actually compile to.

## Backends

| Platform | Backend | Artifact |
| --- | --- | --- |
| Native (`dart.library.io`) | [flutter_rust_bridge](https://pub.dev/packages/flutter_rust_bridge) `=2.13.0` over `crates/cratestack-client-flutter`'s `cbor` module | A **vendored prebuilt native library** — `blobs/linux-x64/libcratestack_client_flutter.so` (Linux), `blobs/android/<abi>/libcratestack_client_flutter.so` (Android: arm64-v8a, x86_64, armeabi-v7a), `blobs/windows-x64/cratestack_client_flutter.dll` (Windows), `macos/Frameworks/CratestackCborNative.xcframework` (macOS, universal arm64 + x86_64), or `ios/Frameworks/CratestackCborNative.xcframework` (iOS, device arm64 + universal simulator arm64/x86_64) in this release. No Rust toolchain, no network fetch, at consumer build time. |
| Web (`dart.library.js_interop`) | The **existing** [`cratestack-cbor-wasm`](../../crates/cratestack-cbor-wasm) wasm-bindgen artifact (already shipped to npm as [`@cratestack/cbor-web`](../../packages/cratestack-cbor-web)) | A **vendored** `wasm-pack --target web` build — `lib/src/web/wasm-pkg/` — loaded at runtime via `dart:js_interop`. No new codec binding; this reuses the exact same Rust wasm-bindgen crate the JS package already binds. |

Linux, Android, Windows, macOS, and iOS share the same flutter_rust_bridge Dart glue (`lib/src/native/rust/`, platform-independent — frb codegen introspects Rust source, not a target triple) but vendor three genuinely different resolution mechanisms: Linux and Windows each resolve the vendored library by an executable-relative path (the two paths differ — no `lib/` subdirectory on Windows, see `windows/CMakeLists.txt`), Android resolves it by bare SONAME from the app's own native library directory (populated by `android/build.gradle`'s `jniLibs.srcDirs`), and macOS/iOS both resolve it by a *fixed* relative framework path with no computation at all — that only works because CocoaPods **links** the vendored xcframework into the built app rather than merely copying it (see `macos/cratestack_cbor.podspec`/`ios/cratestack_cbor.podspec` and `lib/src/native/native_cbor_codec.dart`). The two Apple platforms share that Dart-side resolution function outright (`_resolveMacosLibraryPath`, reused by the iOS branch too) — only the *xcframework's own internal shape* differs between them, not the mechanism: macOS's is a versioned bundle (`Versions/A/...` + symlinks, shipped zipped because `dart pub publish` dereferences symlinks), iOS's is flat/shallow (no symlinks at all, shipped unpacked) — see those two podspecs' header comments.

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

Every platform this package claims is backed by a real prebuilt binary and a
real end-to-end test. The one gap left is **Linux arm64**, and it is not
follow-up work — it is blocked upstream:

- **Native platform matrix:** Linux x86_64, Android (arm64-v8a, x86_64,
  armeabi-v7a), Windows x86_64, macOS (arm64 + x86_64, universal), and iOS
  (device arm64 + universal simulator arm64/x86_64).
  `resolveVendoredLibraryPath()` throws a clear `UnsupportedError` on Linux
  arm64 rather than silently failing.
- **Linux arm64 (Flutter): blocked upstream, not deferred.** Flutter
  publishes **no arm64 Linux SDK on any channel**. Checked directly against
  the release manifest — of 732 entries in
  `https://storage.googleapis.com/flutter_infra_release/releases/releases_linux.json`,
  301 are tagged `dart_sdk_arch: x64` (through 2026-08-19) and the other 431
  predate that field entirely (all dated 2018-02-27 → 2022-01-27, all x64
  tarballs); **zero** archive paths contain `arm` or `aarch`. A throwaway
  spike on a real `ubuntu-24.04-arm` runner confirmed the practical effect:
  the host itself is fine (native `aarch64-unknown-linux-gnu` rustc, and
  clang/cmake/ninja/GTK3/xvfb all install cleanly), but
  `subosito/flutter-action` fails with `Unable to determine Flutter version
  for channel: stable version: any architecture: arm64`, so `flutter build
  linux` never runs. A user cannot reach this package's missing `.so` on
  arm64 Linux without first running a Flutter SDK that does not exist for
  their host. Revisit if Flutter ever publishes arm64 Linux archives.
- **Linux arm64 (plain `dart`): open, and narrower than the above.** The
  **Dart** SDK *does* ship `dartsdk-linux-arm64-release.zip`, so the
  dev-mode `Isolate.resolvePackageUri` path — the one `dart test`/`dart run`
  use, which needs no Flutter bundling at all — is genuinely reachable on
  arm64 Linux today, and throws. Supporting just that case needs only a
  vendored `blobs/linux-arm64/` library; it is tracked separately on
  cratestack#823 and is not what the Flutter block above rules out.

## Flutter app integration — proven, not just `dart test`

`dart test`/`dart test -p chrome` prove the codec works as a Dart *library*.
They do not prove it works inside a real Flutter *app*: a compiled app has no
package source tree for `Isolate.resolvePackageUri` to find, a release web
bundle has no dev server to serve the `packages/...` URL convention from, and
Android/macOS/iOS have no `dart test` story at all. This package closes that
gap for **Linux desktop, Android, Windows desktop, macOS desktop, iOS, and
web**:

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
- **iOS:** `ios/cratestack_cbor.podspec` makes this package a Flutter FFI
  plugin (`pubspec.yaml`'s `flutter: plugin: platforms: ios: ffiPlugin:
  true`) that hands the vendored xcframework (`ios/Frameworks/
  CratestackCborNative.xcframework`, produced by `just cbor-vendor-ios`) to
  **CocoaPods'** `vendored_frameworks` — the SAME mechanism macOS uses, not
  a fourth one: CocoaPods links the vendored framework into the built app,
  so `native_cbor_codec.dart`'s iOS branch resolves it with macOS's exact
  fixed relative string and computes no path at all, sharing that resolution
  function outright rather than duplicating it. The one genuine difference
  is inside the xcframework itself, not the mechanism: iOS uses flat/shallow
  frameworks (device arm64 + a `lipo`'d universal simulator arm64/x86_64
  slice, no `Versions/A/...`, no symlinks) rather than macOS's versioned
  bundle — see `ios/cratestack_cbor.podspec`'s and `just cbor-vendor-ios`'s
  own header comments for why that also means the iOS xcframework ships
  **unpacked**, with no zip/`prepare_command` step (macOS needs one because
  `dart pub publish` dereferences symlinks and a macOS framework has
  mandatory symlinks; a flat iOS framework has none to lose). No dev-mode
  fallback exists either, so — like macOS and Android — the native backend
  has no `dart test` story on iOS; `example/` and `just
  cbor-example-verify-ios` (simulator only — see that recipe's own comment
  for why a real device is out of scope for an unattended CI gate) are the
  only way to exercise it. **Unverified by this repo's own CI/toolchain**,
  same status every Apple-platform slice had before its own first CI run;
  `cratestack-cbor-ios` in `.github/workflows/ci.yml` is this recipe's first
  real execution.
- **Web:** `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm` pair
  as real Flutter assets, so a release `flutter build web` copies them into
  `build/web/assets/packages/cratestack_cbor/...`. `web_cbor_codec.dart` tries
  the dev-server `packages/...` URL first (unchanged, still what `dart test
  -p chrome`/`flutter run -d chrome` use), then falls back to the release
  `assets/packages/.../lib/...` URL — the two conventions coexist, neither
  subsumes the other.

`dart-packages/cratestack_cbor/example/` is a minimal Flutter app proving
all six, with real builds: see that directory's README, `just
cbor-example-verify` (Linux+web, wired into CI as `cratestack-cbor-example`),
`just cbor-example-verify-android` (Android APK build + per-ABI presence
proof, wired into CI as `cratestack-cbor-android`), `just
cbor-example-verify-windows` (Windows `.exe` build + DLL presence proof,
wired into CI as `cratestack-cbor-windows`), `just cbor-example-verify-macos`
(macOS `.app` build + universal-xcframework presence proof, wired into CI as
`cratestack-cbor-macos`), and `just cbor-example-verify-ios` (iOS simulator
`.app` build + xcframework presence proof, wired into CI as
`cratestack-cbor-ios`). Linux, web, Windows, macOS, and iOS all build in
**release** mode (iOS: `--simulator --no-codesign`, sidestepping the
signing identity a real device would need) and actually run the built app —
Linux headless via `xvfb-run`, web served and driven by a real headless
Chrome, Windows and macOS run directly (hosted runners have a real desktop
session, no headless wrapper needed), iOS via a booted simulator (`xcrun
simctl`) — not `flutter run`'s dev server, which resolves assets differently
and would prove nothing about a release deploy. Android additionally has a
**local/manual** companion, `just cbor-example-verify-android-emulator`,
that installs the built APK on a real Android emulator and asserts the app
actually round-trips CBOR at runtime — deliberately not wired into CI, since
booting an emulator on a hosted runner is substantially heavier and flakier
than everything else this package's CI already does; see that recipe's own
comment in the `justfile` for the full reasoning.

Linux arm64 has no entry here because it cannot have one: every platform
above is proven by building and running a real Flutter app, and there is no
arm64 Linux host that can run `flutter build linux` — see the platform matrix
note above for the release-manifest evidence.

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
just cbor-vendor-ios             # release build for all THREE iOS triples (must run ON macOS) -> ios/Frameworks/CratestackCborNative.xcframework (reuses cbor-vendor-native's glue — run cbor-vendor-glue first)
```

See the `justfile` for what each does.

**None of the vendored output is `git`-tracked** — not the Dart glue at
`lib/src/native/rust/`, the native libraries at `blobs/linux-x64/`,
`blobs/android/<abi>/`, and `blobs/windows-x64/`, the assembled xcframeworks
at `macos/Frameworks/` and `ios/Frameworks/`, nor the wasm build at
`lib/src/web/wasm-pkg/`. This matches `CLAUDE.md`'s "don't
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
intentionally NOT empty for exactly one platform (macOS — it excludes the
unpacked xcframework in favor of the zip beside it, see "The macOS framework
ships zipped" in `docs/tooling/dart-publishing.md`); every other gitignored
build output, including iOS's unpacked xcframework, is still packed as-is.
**Verified, not assumed** — `dart pub publish --dry-run` with all five
vendored artifact directories (`blobs/`, `lib/src/web/wasm-pkg/`,
`lib/src/native/rust/`, `macos/Frameworks/`, `ios/Frameworks/`) gitignored
and untracked still lists them all:

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

Total compressed archive size was ~1 MB with the pre-iOS four native
platforms (Linux, Android, Windows, macOS) plus the wasm pair vendored —
still a small fraction of pub.dev's 100 MB gzip recommendation (see
cratestack#563's thread for the original size-budget analysis). Adding iOS's
xcframework (two Mach-O slices, roughly comparable in size to macOS's own)
grows that total somewhat but is not expected to approach the 100 MB
recommendation — this has not been re-measured with a real iOS artifact
(this repo's dev toolchain cannot build one; see `just cbor-vendor-ios`'s own
header comment), so treat the ~1 MB figure as pre-iOS, not current. `git
ls-files` reports zero of the vendored artifacts tracked regardless. So the
published package vendors its binaries exactly as decided, while the
repository stays free of build output.

Regenerate them with the `just` recipes above; treat them as build outputs
to keep in sync with the source crates, never as source to hand-edit.
**`.pubignore` must stay tracked** — without it in a fresh
checkout, publishing silently reverts to dropping these files.

## Verifying both backends

```bash
just cbor-verify-package        # both backends, native (dart test) + web (dart test -p chrome) + flutter test
```

or directly, from this directory:

```bash
dart test                        # native backend only (@TestOn('vm'))
dart test -p chrome              # web backend only (@TestOn('browser'))
flutter test                     # the same VM suite, under a different runtime
```

The third one is not a redundant re-run of the first (cratestack#794).
`flutter test` executes on `flutter_tester`, which does not implement
`Isolate.resolvePackageUriSync` — so the dev-mode library resolution `dart
test` exercises does not merely fail there, it throws `Unsupported operation`,
and the `.dart_tool/package_config.json` fallback runs instead. Before that
fallback existed, this package could not be exercised under `flutter test`
without setting `CRATESTACK_CBOR_NATIVE_LIB`, which is exactly what pushed
consumers into writing a second bootstrap of their own — and *that* is what
collided with this package's own initialization. Run it with the env var
unset, or it proves nothing.

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
