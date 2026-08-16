# Developing `cratestack_cbor`

`dart-packages/cratestack_cbor` is the Dart/Flutter CBOR package (cratestack#563), published to
pub.dev as `cratestack_cbor` under the `cratestack.dev` publisher. It exposes **one** uniform
`CratestackCborCodec` API over **two** completely different backends, chosen by Dart conditional
exports:

| Platform | Backend | Artifact |
| --- | --- | --- |
| Native (`dart.library.io`) | flutter_rust_bridge `=2.12.0` over `crates/cratestack-client-flutter`'s `cbor` module | A prebuilt `.so`/`.dylib`/`.dll` vendored into the published archive |
| Web (`dart.library.js_interop`) | The **existing** `crates/cratestack-cbor-wasm` wasm-bindgen crate (the same one `@cratestack/cbor-web` binds for npm) | A `wasm-pack --target web` build, loaded at runtime |

This mirrors `packages/cratestack-cbor` (`@cratestack/cbor`), which already does native-vs-web
selection behind one import path for JavaScript. There is deliberately **no third binding** of the
codec — both backends compile the same Rust.

> **Status:** Linux x86_64 + web only. Every other platform throws a clear `UnsupportedError`. This
> is a deliberate one-platform spike to validate the vendoring shape before replicating it across
> the ~12-slice matrix. **Do not publish in this state.**
>
> Proven for both `dart test` (as a Dart library) AND a real Flutter app (`flutter build linux` +
> `flutter build web`, release, not the dev server) — see "Verifying inside a real Flutter app"
> below and `dart-packages/cratestack_cbor/example/`.

## Prerequisites

```
flutter                      3.44.1
dart                         3.12.1
flutter_rust_bridge_codegen  2.12.0     # must match the =2.12.0 pin exactly
wasm-pack                               # for the web artifact
google-chrome / chromium                # required to run the web tests for real
```

The frb version pin is exact on both sides. A mismatch between the installed
`flutter_rust_bridge_codegen` and the `flutter_rust_bridge = "=2.12.0"` dependency produces glue
that does not compile.

## First run after cloning

**The vendored artifacts are build output and are not in git.** A fresh clone has no `.so` and no
wasm bundle, so tests fail until you build them:

```bash
just cbor-vendor-native   # frb glue + blobs/linux-x64/*.so
just cbor-vendor-web      # wasm-pack --target web -> lib/src/web/wasm-pkg/
just cbor-verify-package  # runs both backends
```

If you skip this, the failure tells you so explicitly rather than leaving you guessing:

```
Bad state: cratestack_cbor: vendored native library not found at .../blobs/linux-x64/lib….so
If you are working in the cratestack repo, this is expected on a fresh clone — the vendored
artifacts are build output and are not committed. Run:
    just cbor-vendor-native
```

## Four things that will bite you

These are not style preferences. Each one was found the hard way, and each fails in a way that
looks like success.

### 1. `.pubignore` is load-bearing — deleting it silently ships a broken package

`dart pub publish` uses `git ls-files` to decide what goes in the tarball when the package is
inside a Git working tree. Files excluded by `.gitignore` are **silently omitted** — no warning, no
error, and pub has no `files`-field equivalent to opt them back in. Since the vendored binaries are
gitignored, that would publish a package missing the very artifacts it exists to vendor.

`.pubignore` disarms this: when present in a directory, pub consults it *instead of* `.gitignore`
there. This package's `.pubignore` deliberately excludes nothing, so the gitignored build output is
still packed.

Verify with a dry run — it lists what would actually ship:

```bash
cd dart-packages/cratestack_cbor && dart pub publish --dry-run
```

The three artifacts must appear:

```
│       └── libcratestack_client_flutter.so (903 KB)
│           │   ├── cratestack_cbor_wasm.js (21 KB)
│           │   └── cratestack_cbor_wasm_bg.wasm (121 KB)
```

If they do not, **stop** — do not publish. Check `.pubignore` still exists and is tracked.

### 2. `@TestOn` is load-bearing — without it, web tests pass against the *native* backend

Dart conditional exports resolve by **compile target**, not by which file imports them. So
`test/web_cbor_codec_test.dart` without `@TestOn('browser')` compiles against the *native* backend
under a plain `dart test` and reports **false-positive passes** — it looks like the web backend
works when it has never been exercised.

Both test files declare their platform, and must keep doing so:

```dart
@TestOn('vm')       // native — test/native_cbor_codec_test.dart
@TestOn('browser')  // web    — test/web_cbor_codec_test.dart
```

Run them separately; `dart test` alone does **not** cover web:

```bash
dart test            # native only
dart test -p chrome  # web only
```

### 3. `#[frb(sync)]` is mandatory — the async default is *slower than pure Dart*

`crates/cratestack-client-flutter/benches/cbor_bridge/README.md` measures frb's default async
dispatch at **0.5x**, i.e. two times slower than pure-Dart `package:cbor`. With `#[frb(sync)]` the
measured speedup is **~3x** (2.97x/3.48x release, consistent with the 3–4.4x recorded range).

So if you add a bridged function, annotate it. Check the generated Dart: sync bindings return bare
types, async ones return `Future<>`.

```bash
grep -c "Future<" dart-packages/cratestack_cbor/lib/src/native/rust/cbor.dart   # must be 0
```

Note the headline figure in #563's motivation (~55x min / ~1000x avg, measured on a different
stack) is **not** reproduced here. Plan against ~3x.

### 4. Never rustfmt the generated glue

rustfmt's module resolution does not evaluate `#[cfg(...)]` — it tries to locate every `mod x;`
file regardless of the feature gate it sits behind. So `cargo fmt -p cratestack-client-flutter`
fails with ``failed to resolve mod `frb_generated` `` when the glue is absent, and formats 14k
lines of generated output when it is present. Neither is wanted.

`just _fmt` handles this: it stashes any real glue aside, formats the crate's own sources against a
one-line placeholder, and restores the glue (via `trap`, so an interrupted run cannot leave the
placeholder standing in for real glue). You should not need to think about it — but if you see
rustfmt complaining about `frb_generated.rs`, that mechanism is what broke.

### 5. `dart test` proves the library; it does not prove the Flutter app

Everything above is proven under `dart test`/`dart test -p chrome`. Neither exercises the mechanism a
real Flutter app actually uses:

- **Native:** `dart test` loads the vendored `.so` via `Isolate.resolvePackageUri` — that call needs
  this package's *source tree* on disk (a path dependency, or a pub cache checkout), which a compiled
  Flutter app does not have. A real app instead needs Flutter's own plugin-bundling mechanism
  (`dart-packages/cratestack_cbor/linux/CMakeLists.txt`, an FFI plugin per
  `pubspec.yaml`'s `flutter: plugin: platforms: linux: ffiPlugin: true`) to copy the `.so` into the
  built bundle, and `lib/src/native/native_cbor_codec.dart` tries that location first, falling back to
  the `dart test`-only path second.
- **Web:** `dart test -p chrome` and `flutter run -d chrome` both serve this package through a dev
  server with a special `packages/cratestack_cbor/...` URL route. A release `flutter build web` bundle
  has no such route — it needs the `.js`/`.wasm` declared as real Flutter assets
  (`pubspec.yaml`'s `flutter: assets:`), which land at a *different* URL,
  `assets/packages/cratestack_cbor/lib/src/web/wasm-pkg/...` (note: this keeps the `lib/` segment the
  dev-server convention strips — verified against a real release build output, not assumed by
  symmetry). `web_cbor_codec.dart` tries both, in that order.

Both are proven by `dart-packages/cratestack_cbor/example/` — a real Flutter app depending on this
package via a `path:` dependency — and `just cbor-example-verify`, which does real
`flutter build linux`/`flutter build web` builds, actually **runs** the built Linux binary (headless,
via `xvfb-run`) and actually **serves and loads** the built web bundle in a real headless Chrome, and
asserts both print the same CBOR-hex round-trip result as the shared fixtures. See that command's own
comments in the repo's `justfile`.

One more gotcha specific to iterating on the example locally: Flutter's incremental-build cache
(`.dart_tool/flutter_build/`) lives *outside* `build/`. If you ever `rm -rf build` by hand without also
clearing that cache, Flutter's build system trusts its stale "up to date" record and skips regenerating
`build/native_assets/linux/` — an empty directory `linux/CMakeLists.txt` unconditionally installs — so
the next `flutter build linux` fails with a CMake `file INSTALL cannot find ... native_assets/linux`
error that has nothing to do with this package. `flutter clean` (which `just cbor-example-verify`
always runs first) removes both together, so there is nothing to desynchronize.

## Verifying a change

```bash
just cbor-verify-package
```

runs both backends (native 7 tests, web 6 tests in headless Chrome) against the vendored artifacts,
asserting the **same hex fixtures** that `crates/cratestack-cbor-napi`,
`crates/cratestack-client-flutter`, and the npm packages' own suites assert. That shared set is what
makes this a cross-language guarantee rather than internal self-consistency — if you add a fixture,
add it to all of them.

### Prove your test actually tests something

Given how many of the failure modes here look like passes, a green run is weak evidence on its own.
The cheap discriminator is to break the artifact and confirm the suite notices:

```bash
# web: corrupt the wasm -> must fail with a WebAssembly magic-word error
printf 'BROKEN' > dart-packages/cratestack_cbor/lib/src/web/wasm-pkg/cratestack_cbor_wasm_bg.wasm
dart test -p chrome    # expect: CompileError: expected magic word 00 61 73 6d, found 42 52 4f 4b

# native: corrupt the .so -> must fail naming the VENDORED path
printf 'NOT-AN-ELF' > dart-packages/cratestack_cbor/blobs/linux-x64/libcratestack_client_flutter.so
dart test              # expect: Failed to load dynamic library '.../blobs/linux-x64/…': file too short
```

Then `just cbor-vendor-native` / `just cbor-vendor-web` to restore. If a suite still passes with its
artifact corrupted, it is not exercising that backend.

The same applies to the CI drift check: renaming a bridged Rust function must make
`just frb-verify-client-flutter` fail with `Method not found: 'encodeJson'` from the Dart side. The
same discipline applies to the example app — see "Verifying inside a real Flutter app" above and
`just cbor-example-verify`'s own break-it proof.

## CI

Two jobs in `.github/workflows/ci.yml`:

- **`flutter (cratestack_cbor package — native + web)`** installs the pinned toolchain, runs both
  vendor recipes, then `just cbor-verify-package` (the `dart test`/`dart test -p chrome` library-level
  proof above).
- **`flutter (cratestack_cbor example — linux + web, real builds)`** additionally installs the Linux
  desktop toolchain (GTK3 dev headers, cmake, ninja) and `xvfb`, vendors the same artifacts, then runs
  `just cbor-example-verify` — the real `flutter build linux`/`flutter build web` proof. This is the
  single most expensive job in the workflow (a full Linux desktop compile plus a full dart2js web
  compile, on top of the Rust/wasm builds the package job already does); see that job's comments in
  `ci.yml` for the CI-cost tradeoff and why it currently runs unscoped (this workflow has no path
  filtering anywhere yet — this job doesn't invent one unilaterally).

Both build the artifacts fresh every run rather than trusting anything committed — which is the only
option now that nothing is committed.

**Known gap:** there is no byte-level staleness check between a fresh build and any reference.
Rust/wasm builds are not reproducible here (embedded paths, timestamps), so the
`just regen-examples --check` pattern does not transfer — `git diff --exit-code` on a `.so` would
fail spuriously. Behavioural equivalence is what is asserted. A real staleness check needs
reproducible builds and belongs with the publish work.

## Layout

```
dart-packages/cratestack_cbor/
├── .gitignore              # blobs/, wasm-pkg/, native/rust/ — build output; !example/pubspec.lock
├── .pubignore              # MUST stay tracked; see gotcha 1
├── lib/
│   ├── cratestack_cbor.dart          # conditional export: picks a backend
│   └── src/
│       ├── cbor_codec.dart           # the uniform API
│       ├── native/native_cbor_codec.dart
│       ├── native/rust/              # GENERATED frb glue (gitignored)
│       ├── web/web_cbor_codec.dart
│       ├── web/wasm-pkg/             # GENERATED wasm build (gitignored)
│       └── unsupported_cbor_codec.dart
├── blobs/linux-x64/        # GENERATED native library (gitignored)
├── linux/
│   └── CMakeLists.txt      # Flutter FFI-plugin build file — bundles the vendored .so
├── example/                 # real Flutter app proving both backends — see its own README
│   ├── lib/main.dart
│   ├── linux/               # committed (unlike examples/flutter-riverpod, embedded-flutter) —
│   │                         # this example's whole point is `flutter build`, so CI needs it
│   ├── web/
│   └── tool/verify_web_console.dart   # headless-Chrome DevTools Protocol driver
└── test/
    ├── shared_fixtures.dart          # the cross-language fixture set
    ├── native_cbor_codec_test.dart   # @TestOn('vm')
    └── web_cbor_codec_test.dart      # @TestOn('browser')
```

It lives in `dart-packages/`, not `packages/`, for two reasons: `pnpm-workspace.yaml` globs
`packages/*` and every entry there is an npm workspace member, and pub.dev requires the snake_case
name `cratestack_cbor`, which cannot reuse the existing kebab-case `packages/cratestack-cbor` (the
npm umbrella).

## Not done yet

In the order they should land:

1. **Platform matrix** — the remaining ~11 slices (Android ABIs, iOS device+sim, macOS arm64/x64,
   Linux arm64, Windows x64). The Flutter plugin scaffolding pattern itself is now proven for Linux
   (`linux/CMakeLists.txt`, consuming a prebuilt binary — no cargokit); replicating it per platform
   (`macos/`, `windows/`, `android/`, `ios/` — each with their own bundling convention) is the
   remaining work, plus actually building the binaries for each.
2. **pub.dev publish via GitHub Actions OIDC**, version-locked to the workspace version like the npm
   packages.
3. **The generator seam.** `crates/cratestack-client-dart/templates/pubspec.yaml.j2` still emits
   `cbor: ^6.5.1`. It must stay that way until `cratestack_cbor` is actually on pub.dev — the
   template emits real dependencies, so naming an unpublished package breaks `dart pub get` for
   every generated client, including the committed `examples/flutter-riverpod/client` and its drift
   check. **Publish strictly precedes the seam.**

## See also

- `docs/tooling/npm-publishing.md` — the npm/crates.io release pipeline this will eventually sit
  alongside, and the source of the "verify the tarball before publishing" habit in gotcha 1.
- `crates/cratestack-client-flutter/benches/cbor_bridge/README.md` — the benchmark, its real
  numbers, and why the JSON-text boundary caps them.
- `dart-packages/cratestack_cbor/README.md` — the consumer-facing package README that ships to
  pub.dev.
- `dart-packages/cratestack_cbor/example/README.md` — the example app's own README: how to run it,
  and why it lives under `example/` rather than the repo-root `examples/`.
