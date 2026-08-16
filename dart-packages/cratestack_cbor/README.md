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
| Native (`dart.library.io`) | [flutter_rust_bridge](https://pub.dev/packages/flutter_rust_bridge) `=2.12.0` over `crates/cratestack-client-flutter`'s `cbor` module | A **vendored prebuilt native library** — `blobs/linux-x64/libcratestack_client_flutter.so` in this release. No Rust toolchain, no network fetch, at consumer build time. |
| Web (`dart.library.js_interop`) | The **existing** [`cratestack-cbor-wasm`](../../crates/cratestack-cbor-wasm) wasm-bindgen artifact (already shipped to npm as [`@cratestack/cbor-web`](../../packages/cratestack-cbor-web)) | A **vendored** `wasm-pack --target web` build — `lib/src/web/wasm-pkg/` — loaded at runtime via `dart:js_interop`. No new codec binding; this reuses the exact same Rust wasm-bindgen crate the JS package already binds. |

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

This is a **single-platform spike** (cratestack#563), not the full package:

- **Native platform matrix:** Linux x86_64 only. `resolveVendoredLibraryPath()`
  throws a clear `UnsupportedError` on every other platform (macOS, Windows,
  Android, iOS, Linux arm64) rather than silently failing. The full ~12-slice
  matrix is deliberate follow-up work, not an oversight.
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
package source tree for `Isolate.resolvePackageUri` to find, and a release
web bundle has no dev server to serve the `packages/...` URL convention from.
This package closes that gap for **Linux desktop and web**:

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
- **Web:** `pubspec.yaml`'s `flutter: assets:` vendors the `.js`/`.wasm` pair
  as real Flutter assets, so a release `flutter build web` copies them into
  `build/web/assets/packages/cratestack_cbor/...`. `web_cbor_codec.dart` tries
  the dev-server `packages/...` URL first (unchanged, still what `dart test
  -p chrome`/`flutter run -d chrome` use), then falls back to the release
  `assets/packages/.../lib/...` URL — the two conventions coexist, neither
  subsumes the other.

`dart-packages/cratestack_cbor/example/` is a minimal Flutter app proving
both, with real builds: see that directory's README and `just
cbor-example-verify` (also wired into CI as `cratestack-cbor-example`). It
builds the example for Linux and web in **release** mode, actually runs the
Linux binary headless, and actually serves and loads the web release bundle
in a real headless Chrome — not `flutter run`'s dev server, which resolves
assets differently and would prove nothing about a release deploy.

Android and iOS remain out of scope for this slice (deliberately — see the
platform matrix note above).

## Regenerating the vendored artifacts

> **Working on this package rather than using it?** Read
> [docs/tooling/cratestack-cbor-development.md](https://github.com/cratestack/cratestack/blob/main/docs/tooling/cratestack-cbor-development.md)
> first. It covers the toolchain pins, the first-run steps, and four
> failure modes that each *look like success* — a missing `.pubignore`
> silently publishing a package without its binaries, a web test that
> passes against the native backend, a bridged function that is 2x slower
> than pure Dart because it is async, and rustfmt reformatting generated
> glue.

Both artifacts are build outputs from crates in this repo and are
regenerated, not hand-written. From the repository root:

```bash
just cbor-vendor-native   # flutter_rust_bridge glue + blobs/linux-x64/*.so
just cbor-vendor-web      # wasm-pack --target web build -> lib/src/web/wasm-pkg/
```

See the `justfile` for what each does.

**None of the vendored output is `git`-tracked** — not the Dart glue at
`lib/src/native/rust/`, the native library at `blobs/linux-x64/`, nor the
wasm build at `lib/src/web/wasm-pkg/`. This matches `CLAUDE.md`'s "don't
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
all three directories gitignored and untracked still lists them:

```
│       └── libcratestack_client_flutter.so (903 KB)
│           │   ├── cratestack_cbor_wasm.js (21 KB)
│           │   └── cratestack_cbor_wasm_bg.wasm (121 KB)
```

Archive size is identical either way (502 KB), and `git ls-files` reports
zero of them tracked. So the published package vendors its binaries exactly
as decided, while the repository stays free of build output.

Regenerate them with the two `just` recipes above; treat them as build
outputs to keep in sync with the source crates, never as source to
hand-edit. **`.pubignore` must stay tracked** — without it in a fresh
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
