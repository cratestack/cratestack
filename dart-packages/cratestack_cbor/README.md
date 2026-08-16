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
| Native (`dart.library.io`) | [flutter_rust_bridge](https://pub.dev/packages/flutter_rust_bridge) `=2.12.0` over `crates/cratestack-client-flutter`'s `cbor` module | A **vendored prebuilt native library** — `blobs/linux-x64/libcratestack_client_flutter.so` (Linux) or `blobs/android/<abi>/libcratestack_client_flutter.so` (Android: arm64-v8a, x86_64, armeabi-v7a) in this release. No Rust toolchain, no network fetch, at consumer build time. |
| Web (`dart.library.js_interop`) | The **existing** [`cratestack-cbor-wasm`](../../crates/cratestack-cbor-wasm) wasm-bindgen artifact (already shipped to npm as [`@cratestack/cbor-web`](../../packages/cratestack-cbor-web)) | A **vendored** `wasm-pack --target web` build — `lib/src/web/wasm-pkg/` — loaded at runtime via `dart:js_interop`. No new codec binding; this reuses the exact same Rust wasm-bindgen crate the JS package already binds. |

Linux and Android share the same flutter_rust_bridge Dart glue (`lib/src/native/rust/`, platform-independent — frb codegen introspects Rust source, not a target triple) but vendor genuinely different resolution mechanisms: Linux resolves the vendored library by an executable-relative path, Android resolves it by bare SONAME from the app's own native library directory (populated by `android/build.gradle`'s `jniLibs.srcDirs`) — see `lib/src/native/native_cbor_codec.dart`.

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

- **Native platform matrix:** Linux x86_64 and Android (arm64-v8a, x86_64,
  armeabi-v7a) only. `resolveVendoredLibraryPath()` throws a clear
  `UnsupportedError` on every other platform (macOS, Windows, iOS, Linux
  arm64) rather than silently failing. The remaining matrix is deliberate
  follow-up work, not an oversight.
- **Not published to pub.dev.** `pubspec.yaml` declares `publish_to: none`
  deliberately. Publishing is a separate, maintainer-gated step (verified
  publisher `cratestack.dev`, GitHub Actions OIDC — see cratestack#563's
  issue thread).
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
Android has no `dart test` story at all. This package closes that gap for
**Linux desktop, Android, and web**:

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
- **Web:** `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm` pair
  as real Flutter assets, so a release `flutter build web` copies them into
  `build/web/assets/packages/cratestack_cbor/...`. `web_cbor_codec.dart` tries
  the dev-server `packages/...` URL first (unchanged, still what `dart test
  -p chrome`/`flutter run -d chrome` use), then falls back to the release
  `assets/packages/.../lib/...` URL — the two conventions coexist, neither
  subsumes the other.

`dart-packages/cratestack_cbor/example/` is a minimal Flutter app proving all
three, with real builds: see that directory's README, `just
cbor-example-verify` (Linux+web, wired into CI as `cratestack-cbor-example`),
and `just cbor-example-verify-android` (Android APK build + per-ABI presence
proof, wired into CI as `cratestack-cbor-android`). Linux and web build in
**release** mode, actually run the Linux binary headless, and actually serve
and load the web release bundle in a real headless Chrome — not `flutter
run`'s dev server, which resolves assets differently and would prove nothing
about a release deploy. Android additionally has a **local/manual**
companion, `just cbor-example-verify-android-emulator`, that installs the
built APK on a real Android emulator and asserts the app actually round-trips
CBOR at runtime — deliberately not wired into CI, since booting an emulator
on a hosted runner is substantially heavier and flakier than everything else
this package's CI already does; see that recipe's own comment in the
`justfile` for the full reasoning.

iOS, macOS, Windows, and Linux arm64 remain out of scope for this slice
(deliberately — see the platform matrix note above).

## Regenerating the vendored artifacts

> **Working on this package rather than using it?** Read
> [docs/tooling/cratestack-cbor-development.md](https://github.com/cratestack/cratestack/blob/main/docs/tooling/cratestack-cbor-development.md)
> first. It covers the toolchain pins, the first-run steps, and four
> failure modes that each *look like success* — a missing `.pubignore`
> silently publishing a package without its binaries, a web test that
> passes against the native backend, a bridged function that is 2x slower
> than pure Dart because it is async, and rustfmt reformatting generated
> glue.

All three artifacts are build outputs from crates in this repo and are
regenerated, not hand-written. From the repository root:

```bash
just cbor-vendor-native   # flutter_rust_bridge glue + blobs/linux-x64/*.so
just cbor-vendor-web      # wasm-pack --target web build -> lib/src/web/wasm-pkg/
just cbor-vendor-android  # cargo ndk cross-compile -> blobs/android/<abi>/*.so (reuses cbor-vendor-native's glue — run that first)
```

See the `justfile` for what each does.

**None of the vendored output is `git`-tracked** — not the Dart glue at
`lib/src/native/rust/`, the native libraries at `blobs/linux-x64/` and
`blobs/android/<abi>/`, nor the wasm build at `lib/src/web/wasm-pkg/`. This matches `CLAUDE.md`'s "don't
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
