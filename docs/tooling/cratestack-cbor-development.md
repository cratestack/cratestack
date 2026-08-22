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

> **Status:** Linux x86_64, Android (arm64-v8a, x86_64, armeabi-v7a), Windows x86_64, macOS (arm64 +
> x86_64, universal), and web. Every other platform throws a clear `UnsupportedError`. iOS and Linux
> arm64 are a deliberate maintainer hold, not in-progress work — see cratestack#563's issue thread.
> **This is the intended platform scope for the first pub.dev publish**, not a partial state to wait
> out. Publishing infrastructure (CI workflow, version-locking, archive-verification gate) now
> exists — see `docs/tooling/dart-publishing.md` for what's left: a one-time manual first publish
> only a maintainer can perform, since pub.dev cannot automate the first version of a brand-new
> package.
>
> Proven for both `dart test` (as a Dart library) AND a real Flutter app — `flutter build linux` +
> `flutter build web` (release, not the dev server), a real `flutter build apk` installed and run
> on a real Android emulator, a real `flutter build windows`, and a real `flutter build macos` — see
> "Verifying inside a real Flutter app" below and `dart-packages/cratestack_cbor/example/`. **The
> Windows and macOS slices are unverified by this repo's own toolchain** (Linux-only — see the Linux
> prerequisite note in this repo's `CLAUDE.md`); `cratestack-cbor-windows`/`cratestack-cbor-macos` in
> `.github/workflows/ci.yml` are their first real execution.

## Prerequisites

```
flutter                      3.44.1
dart                         3.12.1
flutter_rust_bridge_codegen  2.12.0     # must match the =2.12.0 pin exactly
wasm-pack                               # for the web artifact
google-chrome / chromium                # required to run the web tests for real
cargo-ndk                    4.1.2      # for the Android artifact — cargo install cargo-ndk
Android SDK + NDK 28.2.13676358         # matches Flutter's own default ndkVersion; ANDROID_HOME and
                                         # ANDROID_NDK_HOME must be set explicitly (not assumed by
                                         # flutter/adb being on PATH)
```

The frb version pin is exact on both sides. A mismatch between the installed
`flutter_rust_bridge_codegen` and the `flutter_rust_bridge = "=2.12.0"` dependency produces glue
that does not compile.

**The full Flutter SDK is required — a standalone Dart SDK is not enough.** Since this became a
Flutter plugin, its pubspec declares `flutter.plugin.platforms`, which obliges an
`environment.flutter` constraint (pub's publish validator rejects the former without the latter).
A standalone Dart SDK cannot satisfy it:

```
Because cratestack_cbor requires the Flutter SDK, version solving failed.
```

This is easy to miss locally, and it reached CI once for exactly that reason: the Flutter SDK
**bundles its own `dart`**, so on a developer machine `dart pub get` quietly resolves the constraint
from the ambient Flutter install and everything looks fine. Only a genuinely standalone Dart SDK
reproduces it — which is what `dart-lang/setup-dart` installs, and why both CI jobs for this package
use `subosito/flutter-action` instead.

## First run after cloning

**The vendored artifacts are build output and are not in git.** A fresh clone has no `.so` and no
wasm bundle, so tests fail until you build them:

```bash
just cbor-vendor-native   # frb glue + blobs/linux-x64/*.so
just cbor-vendor-web      # wasm-pack --target web -> lib/src/web/wasm-pkg/
just cbor-vendor-android  # cargo ndk cross-compile -> blobs/android/<abi>/*.so (needs cbor-vendor-native's glue first)
just cbor-verify-package  # runs the Linux+web backends (dart test / dart test -p chrome)
```

Android has no `dart test` story of its own (see gotcha 6 below) — verify it with a real
`flutter build apk`:

```bash
just cbor-example-verify-android            # builds the APK, asserts the .so is inside for every ABI
just cbor-example-verify-android-emulator   # installs + runs it on a connected device/emulator (local only)
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

### 6. Android needs a genuinely different resolution mechanism, and a build that ships no library still succeeds

Android is not "Linux with a different `.so` extension" — two things are actually different, not
just relocated:

- **No path to compute.** Linux resolves its vendored library by an executable-relative path
  (`linux/CMakeLists.txt`'s bundling contract). Android's dynamic linker instead resolves a bundled
  library by bare **SONAME** from the app's own native library directory, populated at install time
  from `android/build.gradle`'s `jniLibs.srcDirs` — so `native_cbor_codec.dart`'s Android branch just
  calls `DynamicLibrary.open('libcratestack_client_flutter.so')` with no path logic at all. There is
  also no dev-mode fallback to try: `dart test` does not run on an Android target, so the Flutter-app
  path is the *only* path Android ever exercises — unlike Linux, which has two.
- **A build that ships no library still compiles and reports success.** `flutter build apk` does not
  fail if `android/build.gradle`'s `jniLibs.srcDirs` finds nothing to package — Gradle just quietly
  emits an APK with no native library inside. Verified directly in this repo's own PR: with
  `blobs/android/` deleted, `flutter build apk` still printed `✓ Built ... app-release.apk (43.5MB)`.
  This is why `just cbor-example-verify-android` unzips the built APK and asserts
  `lib/<abi>/libcratestack_client_flutter.so` is actually present for every claimed ABI, the same
  "assert it's inside the built bundle" discipline `cbor-example-verify` already applies to Linux —
  and why that assertion, not "the build exited 0", is the real CI gate.

Two implementation gotchas worth recording separately, since both were found the hard way while
building this check:

- **`unzip -l "$apk" | grep -q pattern` under `set -o pipefail` can report failure ON A MATCH.**
  `grep -q` exits as soon as it finds its first match, which can deliver `unzip` a SIGPIPE before it
  finishes writing; under `pipefail` that non-zero `unzip` exit status propagates as the *pipeline's*
  status even though `grep` itself matched successfully. `just cbor-example-verify-android` captures
  the listing into a variable first and does a plain bash `[[ "$listing" == *pattern* ]]` membership
  test instead, which has no subprocess/pipe in the check at all.
- **The host `strip` does not reliably handle cross-arch AArch64 ELF.** `just cbor-vendor-android`
  uses the Android NDK's own `llvm-strip` (`$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/
  bin/llvm-strip`) rather than the host `strip` binary Linux vendoring uses — GNU `strip`'s
  `--strip-unneeded` does not list an AArch64 Android target among its supported output formats.

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

> **When you do this, make sure the thing you broke is the thing that gets run.** A corrupt-artifact
> check only means something if the build under test was produced *after* the corruption. This bit
> for real: `cbor-example-verify-android-emulator` used to install whatever APK was already on disk,
> so corrupting a vendored `.so` and running it reported a cheerful round-trip from the **stale**
> pre-corruption APK — a green that actively misleads. The tell was the clock: the run finished 18
> seconds after the previous one, far too fast to have rebuilt anything. That recipe now rebuilds
> unconditionally. If you add another verification recipe, make it own its build rather than trusting
> a caller to have run one first.

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

Android has two independent break-it proofs, because presence and validity are checked by two
different recipes (see gotcha 6):

```bash
# presence: delete the vendored ABI directory entirely -> build still succeeds, presence check must fail
mv dart-packages/cratestack_cbor/blobs/android /tmp/blobs-android-backup
just cbor-example-verify-android
# expect: "✓ Built ... app-release.apk" followed by
#         "FAIL: vendored library not found inside the built APK for ABI arm64-v8a ..."
mv /tmp/blobs-android-backup dart-packages/cratestack_cbor/blobs/android

# validity: corrupt one ABI's .so -> the APK still builds AND still "has" the file (unzip -l sees an
# entry), so only running the app on that ABI catches it — install + run on a matching device/emulator:
printf 'NOT-AN-ELF' > dart-packages/cratestack_cbor/blobs/android/x86_64/libcratestack_client_flutter.so
just cbor-example-verify-android
just cbor-example-verify-android-emulator
# expect (on an x86_64 emulator): CRATESTACK_CBOR_EXAMPLE_RESULT: FAILED ... dlopen failed: "/data/app/
# ~~.../base.apk!/lib/x86_64/libcratestack_client_flutter.so" has bad ELF magic: 4e4f542d — note the
# failure names the path INSIDE the installed APK, not a build-tree path, which is what proves this is
# the real plugin mechanism rather than some dev-mode fallback answering instead
just cbor-vendor-android   # restore
```

## CI

Five jobs in `.github/workflows/ci.yml`:

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
- **`flutter (cratestack_cbor android — APK build + jniLibs proof)`** installs `cargo-ndk` (pinned
  `=4.1.2`), the three Android rustup targets, and NDK 28.2.13676358 (matching Flutter's own default —
  GitHub's runner image ships an SDK but not reliably that exact NDK version), then vendors and runs
  `just cbor-example-verify-android` — a real `flutter build apk` plus the "is the library actually
  inside the APK per ABI" assertion (gotcha 6). **Deliberately does not boot an emulator** — see that
  job's own comment in `ci.yml` for the full reasoning, summarized: an NDK cross-compile + Gradle build
  is bounded cost, but a hosted-runner emulator (KVM availability, boot time, flakiness) is
  substantially heavier again, so the on-device round-trip proof
  (`just cbor-example-verify-android-emulator`) stays a **local/manual** gate. This means CI proves the
  library is *present*, and a human running the emulator recipe is what proves it is *valid and
  loadable* — the two break-it proofs in the previous section are deliberately split the same way.
- **`flutter (cratestack_cbor windows — real build, DLL + round-trip proof)`** runs on
  `windows-latest`, installs `flutter_rust_bridge_codegen` (pinned `=2.12.0`) and pinned `binaryen`,
  then `just cbor-vendor-glue` + `just cbor-vendor-lib windows-x64` + `just cbor-vendor-web` (the wasm
  pair must be vendored here too — `pubspec.yaml`'s `flutter: assets:` is unconditional and Flutter has
  no per-platform asset conditionals) followed by `just cbor-example-verify-windows` — a real
  `flutter build windows`, an assertion the vendored `.dll` landed beside the built `.exe`, and a
  round-trip proof from the running executable's captured stdout. This is this repo's own toolchain's
  first-ever real execution of anything Windows-targeted (Linux-only dev machine — see this repo's
  `CLAUDE.md`), so this job doubles as the first genuine verification of the Windows slice, not just a
  regression gate.
- **`flutter (cratestack_cbor macos — real build, xcframework + round-trip proof)`** runs on
  `macos-latest`, mirroring the Windows job's shape but proving a genuinely different mechanism (see
  `macos/cratestack_cbor.podspec`'s header comment): installs both `aarch64-apple-darwin` and
  `x86_64-apple-darwin` rustup targets, `flutter_rust_bridge_codegen` (pinned `=2.12.0`) and pinned
  `binaryen`, then `just cbor-vendor-glue` + `just cbor-vendor-macos` (builds both Darwin arches,
  `lipo`s them into one universal binary, assembles a versioned `.framework`, then an `.xcframework` —
  see that recipe's own comment) + `just cbor-vendor-web`, followed by `just cbor-example-verify-macos`
  — which first **deletes the unpacked xcframework**, so the build is forced through the same
  `prepare_command`-unpacks-the-zip path a pub.dev consumer takes (see "The macOS framework ships
  zipped" in `dart-publishing.md`), then a real `flutter build macos`, an assertion the vendored
  xcframework landed inside the built `.app`'s `Contents/Frameworks/`, an assertion the reconstructed
  framework still has its symlinks, a `lipo -info` check that both arches are actually present in the
  embedded binary, and a round-trip proof from the running app's captured stdout. Also this repo's toolchain's
  first-ever real execution of anything macOS-targeted; the exact command sequence was validated once
  already, on a throwaway spike branch (`spike/cbor-macos-xcframework`,
  `.github/workflows/spike-cbor-macos.yml`) kept around for reference, before being turned into the real
  `just cbor-vendor-macos`/`cbor-example-verify-macos` recipes this job actually runs.

All five build the artifacts fresh every run rather than trusting anything committed — which is the
only option now that nothing is committed.

**Known gap:** there is no byte-level staleness check between a fresh build and any reference.
Rust/wasm builds are not reproducible here (embedded paths, timestamps), so the
`just regen-examples --check` pattern does not transfer — `git diff --exit-code` on a `.so` would
fail spuriously. Behavioural equivalence is what is asserted. A real staleness check needs
reproducible builds and belongs with the publish work.

## Layout

```
dart-packages/cratestack_cbor/
├── .gitignore              # blobs/, macos/Frameworks/, wasm-pkg/, native/rust/ — build output;
│                             # !example/pubspec.lock
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
├── blobs/
│   ├── linux-x64/           # GENERATED native library (gitignored)
│   ├── windows-x64/         # GENERATED native library (gitignored)
│   ├── macos-arm64/         # GENERATED per-arch native library (gitignored) — intermediate input
│   ├── macos-x64/           # to `just cbor-vendor-macos`'s xcframework assembly, not itself shipped
│   └── android/<abi>/       # GENERATED native libraries, one per ABI (gitignored) —
│                             # arm64-v8a, x86_64, armeabi-v7a; matches cargo-ndk's own -o layout,
│                             # which is also exactly Android's jniLibs.srcDirs source-set layout
├── linux/
│   └── CMakeLists.txt      # Flutter FFI-plugin build file — bundles the vendored .so
├── windows/
│   └── CMakeLists.txt      # Flutter FFI-plugin build file — bundles the vendored .dll
├── macos/
│   ├── cratestack_cbor.podspec   # Flutter FFI-plugin build file — CocoaPods vendored_frameworks
│   │                               # + prepare_command, which unpacks the zip below at pod-install time
│   └── Frameworks/               # GENERATED universal xcframework (gitignored, NOT under blobs/ —
│                                   # see the podspec's own header comment for why), plus the
│                                   # .xcframework.zip that is what ACTUALLY ships: pub dereferences
│                                   # symlinks and a symlink-less macOS framework fails codesign, so
│                                   # .pubignore excludes the directory and ships the zip instead
├── android/
│   ├── build.gradle        # Flutter FFI-plugin build file — packages blobs/android/<abi>/*.so via jniLibs
│   └── src/main/AndroidManifest.xml
├── example/                 # real Flutter app proving all five backends — see its own README
│   ├── lib/main.dart
│   ├── linux/               # committed (unlike examples/flutter-riverpod, embedded-flutter) —
│   │                         # this example's whole point is `flutter build`, so CI needs it
│   ├── windows/              # committed for the same reason
│   ├── macos/                # committed for the same reason — Xcode project scaffolding only
│   │                         # (`flutter create --platforms=macos`); NO committed Podfile, because
│   │                         # generating one needs Xcode, which this repo's own toolchain does not
│   │                         # have (see docs/tooling/cratestack-cbor-development.md's own notes on
│   │                         # this) — `flutter pub get`/`flutter build macos` on a real macOS host
│   │                         # generates it fresh from a Flutter SDK template
│   ├── android/              # committed for the same reason (build.gradle.kts, AndroidManifest.xml,
│   │                         # etc. — NOT .gradle/, local.properties, gradlew*, gradle-wrapper.jar:
│   │                         # Flutter regenerates those from its own SDK cache, same convention
│   │                         # `flutter create` itself uses)
│   ├── web/
│   └── tool/verify_web_console.dart   # headless-Chrome DevTools Protocol driver
└── test/
    ├── shared_fixtures.dart          # the cross-language fixture set
    ├── native_cbor_codec_test.dart   # @TestOn('vm') — Linux only; Android/macOS have no
    │                                   # `dart test` story
    └── web_cbor_codec_test.dart      # @TestOn('browser')
```

It lives in `dart-packages/`, not `packages/`, for two reasons: `pnpm-workspace.yaml` globs
`packages/*` and every entry there is an npm workspace member, and pub.dev requires the snake_case
name `cratestack_cbor`, which cannot reuse the existing kebab-case `packages/cratestack-cbor` (the
npm umbrella).

## Not done yet

In the order they should land:

1. **Platform matrix** — Linux x64, web, Android (arm64-v8a, x86_64, armeabi-v7a), Windows x64, and
   macOS (arm64 + x86_64, universal) are done (the Windows and macOS slices' real `flutter build`
   runs are both unverified by this repo's own Linux-only toolchain — their first real execution is
   `cratestack-cbor-windows`/`cratestack-cbor-macos` in `.github/workflows/ci.yml`, on
   `windows-latest`/`macos-latest`). The remaining slice: iOS device+simulator, Linux arm64. The
   Flutter plugin scaffolding pattern is now proven for four genuinely different bundling mechanisms —
   Linux's and Windows' executable-relative bundle paths (`linux/CMakeLists.txt`,
   `windows/CMakeLists.txt` — same `<plugin>_bundled_libraries` contract, two different install
   destinations), Android's SONAME-resolved jniLibs (`android/build.gradle`), and macOS's CocoaPods
   `vendored_frameworks` (`macos/cratestack_cbor.podspec` — a *linked*, not merely copied, xcframework,
   which is what lets the Dart side resolve it with a fixed relative string and no path computation at
   all; see that podspec's and `native_cbor_codec.dart`'s own comments) — so `ios/` is the remaining
   scaffolding work, plus actually building the binaries. Note iOS uses the same xcframework convention
   macOS just proved here (verified on a real `macos-latest` runner — see `spike/cbor-macos-xcframework`
   — even though this repo's own dev machine cannot verify either, having no Xcode).
2. **pub.dev publish via GitHub Actions OIDC**, version-locked to the workspace version like the npm
   packages — the CI workflow (`publish-pubdev-cbor` in `release-cli.yml`) and version-lockstep
   (`just bump` now rewrites this package's `pubspec.yaml` too) both exist as of cratestack#563's
   publish slice; see `docs/tooling/dart-publishing.md`. **What does not exist yet is the first real
   publish** — pub.dev cannot automate a brand-new package's first version (verified against
   dart.dev's own docs), so `cratestack_cbor` 0.8.0 is still unpublished until a maintainer runs the
   manual bootstrap that document describes and enables Automated publishing on pub.dev's Admin tab.
   Until then `publish-pubdev-cbor` fails on every tag push by design (same "hard cutover, not a
   soft-skip" as the npm jobs) rather than quietly no-op'ing.
3. **The generator seam.** `crates/cratestack-client-dart/templates/pubspec.yaml.j2` still emits
   `cbor: ^6.5.1`. It must stay that way until `cratestack_cbor` is actually on pub.dev — the
   template emits real dependencies, so naming an unpublished package breaks `dart pub get` for
   every generated client, including the committed `examples/flutter-riverpod/client` and its drift
   check. **Publish strictly precedes the seam** — still blocked on (2) above.

## See also

- `docs/tooling/dart-publishing.md` — the pub.dev publish workflow, version-locking, the one-time
  manual first-publish a maintainer must perform, and proof that the archive-verification gate is
  load-bearing (not decorative): with the vendored `.so` files deleted, `dart pub publish --dry-run`
  reports the exact same single warning as a fully-vendored package.
- `docs/tooling/npm-publishing.md` — the npm/crates.io release pipeline this sits alongside, and the
  source of the "verify the tarball before publishing" habit in gotcha 1.
- `crates/cratestack-client-flutter/benches/cbor_bridge/README.md` — the benchmark, its real
  numbers, and why the JSON-text boundary caps them.
- `dart-packages/cratestack_cbor/README.md` — the consumer-facing package README that ships to
  pub.dev.
- `dart-packages/cratestack_cbor/example/README.md` — the example app's own README: how to run it,
  and why it lives under `example/` rather than the repo-root `examples/`.
