# pub.dev publishing setup (`cratestack_cbor`, `cratestack_annotations`, `cratestack_builder`)

`dart-packages/cratestack_cbor` (cratestack#563) is the third registry `.github/workflows/release-cli.yml`
publishes to on every `vX.Y.Z` tag push, alongside crates.io and npm — see
[`docs/tooling/npm-publishing.md`](npm-publishing.md) for those two. Read
[`docs/tooling/cratestack-cbor-development.md`](cratestack-cbor-development.md) first: it covers the
package's toolchain pins, the four "looks like success but isn't" failure modes, and the vendoring
recipes (`just cbor-vendor-native`, `cbor-vendor-web`, `cbor-vendor-android`) this document assumes you
already understand.

## The constraint this whole setup exists to satisfy

The vendored native/wasm binaries (`blobs/`, `lib/src/web/wasm-pkg/`, `lib/src/native/rust/`) are
gitignored build output — never in the repository (maintainer decision, cratestack#563: generated in
CI, not committed). `.pubignore` is what lets `dart pub publish` ship them anyway despite being
gitignored (pub consults `.pubignore` instead of `.gitignore` when present — see the development doc's
gotcha 1). **This means any publish workflow MUST vendor every artifact set itself, immediately
before publishing** — there is no committed fallback, and pub.dev gives **no signal** if a vendoring
step is skipped or fails partway (see "Proof the archive-verification gate can fail" below).
`release-cli.yml` does this across **four jobs**, not one, since the macOS/Windows platform-matrix slice
(and later the iOS slice) landed: `publish-pubdev-cbor` itself installs toolchain → vendors the
Linux/Android/web artifact sets (the ones `ubuntu-latest` can build without cross-compiling) → downloads
the macOS xcframework, iOS xcframework, and Windows `.dll` built by the separate
`build-cbor-macos`/`build-cbor-ios`/`build-cbor-windows` jobs (which run on
`macos-latest`/`macos-latest`/`windows-latest`, the only hosts that can produce them) → verifies the
archive → publishes, every release, unconditionally. See those three jobs' and `publish-pubdev-cbor`'s own
comments in `release-cli.yml` for the full per-step reasoning; not repeated here.

## Platform status at time of writing

Linux x86_64, Android (arm64-v8a, x86_64, armeabi-v7a), Windows x86_64, and web are vendored into the
published archive; macOS (arm64+x86_64, as one universal xcframework) landed in the platform-matrix slice
and iOS (device arm64 + universal simulator arm64/x86_64, as one xcframework) landed in a later slice — both
are wired into the release job below. Linux arm64 is the one platform **not** in the vendored archive —
every other platform throws `UnsupportedError` at runtime. This mirrors what the package's own
`pubspec.yaml` and README already say — see
[`docs/tooling/cratestack-cbor-development.md`](cratestack-cbor-development.md)'s own "Status" line for
the authoritative, more frequently updated statement of which platforms are actually done, since that
detail changes faster than this document does.

## Version locking

`cratestack_cbor`'s pub.dev version is locked to the workspace version, the same way every
`packages/*/package.json` already is for npm:

- `just bump X.Y.Z` now also rewrites `dart-packages/cratestack_cbor/pubspec.yaml`'s `version:` field
  (a plain, one-level-deep glob — `dart-packages/*/pubspec.yaml` — matching the existing
  `packages/*/package.json` convention, and deliberately **not** reaching
  `dart-packages/cratestack_cbor/example/pubspec.yaml`, which is an application target with its own
  independent `1.0.0+1` build-number scheme, not a published package).
- **This was a real gap before cratestack#563's publish slice**: `just bump` touched every crate's
  `Cargo.toml` and every npm `package.json`, but had never heard of `dart-packages/`. Fixed as part of
  this work — see `justfile`'s `bump` recipe.
- `just release VERSION` (the local/manual release path) and `prepare-release.yml` (the CI "Prepare
  Release" workflow) both stage and commit `dart-packages/*/pubspec.yaml` alongside the other
  version-bump files, for the identical reason the `packages/*/package.json` glob was added there
  (cratestack#581): a bump that rewrites the file in the working tree but never stages it silently
  ships a release where the file lags the workspace version, and the next tag's publish then hits an
  **unrecoverable** conflict — pub.dev, unlike `publish-crates`, has no "already published, skip"
  tolerance built into this workflow.

## First publish (0.8.0): manual, by a maintainer, before any of this runs

**Verified against [dart.dev/tools/pub/automated-publishing](https://dart.dev/tools/pub/automated-publishing):**

> "Today, you can only automate publishing of existing packages. To create a new package, you must
> publish the first version using `dart pub publish`."

This is a hard constraint of pub.dev itself, not a limitation of this workflow — `publish-pubdev-cbor`
in `release-cli.yml` **cannot** publish `cratestack_cbor` for the first time, the same way
`publish-npm-cbor-node` cannot bootstrap a brand-new npm platform subpackage name (see
`npm-publishing.md`'s identical constraint for that case). The ordering is forced:

1. **A maintainer publishes `cratestack_cbor` 0.8.0 by hand**, from a machine with the full toolchain
   (see `cratestack-cbor-development.md`'s Prerequisites) and pub.dev credentials:

   ```bash
   cd dart-packages/cratestack_cbor
   just cbor-vendor-native
   just cbor-vendor-web
   just cbor-vendor-android      # finds the Android SDK/NDK itself; see below if it can't
   dart pub publish --dry-run    # READ THE OUTPUT — see "Verify before publishing" below
   dart pub publish              # real, interactive; irreversible
   ```

   `cbor-vendor-android` locates the SDK itself — `$ANDROID_HOME`, then `$ANDROID_SDK_ROOT`, then
   `~/Android/Sdk`, then `~/Library/Android/sdk` — and picks the newest NDK under `<sdk>/ndk/`. It
   prints which it chose. If it cannot find either, it says so with the paths it tried; export
   `ANDROID_HOME` (and optionally `ANDROID_NDK_HOME`) and re-run.

   > This step used to demand both variables and abort with `ANDROID_HOME is not set`. That fired
   > during a real first-publish run, *after* the native and web artifacts had already been vendored
   > — the worst moment for a stop, and in the middle of an irreversible sequence. The variables were
   > mentioned in the docs, but as a trailing pointer to another file, which is not where anyone is
   > looking when a command stops mid-flow.

   `dart pub publish` (no `--force`) prompts for confirmation and requires an interactive `dart pub
   login` (or an already-authenticated pub credentials file) — this cannot be scripted or delegated,
   same class of manual step as npm's EOTP 2FA prompt in `npm-publishing.md`.

2. **Only after that first publish succeeds**, the maintainer opens `cratestack_cbor`'s Admin tab on
   pub.dev (`pub.dev/packages/cratestack_cbor/admin`) and enables **Automated publishing**:
   - **Repository:** `cratestack/cratestack`
   - **Tag pattern:** `v{{version}}` (matches this repo's `vX.Y.Z` tags exactly — the workspace version
     the pub.dev package is now locked to)
   - **Environment:** leave unset (not used here, same choice `npm-publishing.md` makes for npm's
     equivalent optional field)

   This step is **pub.dev-side configuration a human must perform in a browser** — nothing in this
   repository's CI can do it, the same way nothing in `release-cli.yml` can attach npm's Trusted
   Publisher to a brand-new package name.

3. **From the next tag push onward** (i.e. `v0.8.1` and later), `publish-pubdev-cbor` in
   `release-cli.yml` runs automatically and needs no further manual step, the same as any npm job
   with a Trusted Publisher already configured.

Until step 2 is complete, `publish-pubdev-cbor` will fail on every tag push (pub.dev rejects the OIDC
exchange for a package with no Automated publishing configuration) — this is intentional: same
"hard cutover, not a soft-skip" design as the npm jobs (see `npm-publishing.md`), so a missing
configuration fails loudly instead of quietly no-op'ing.

## What the CI job actually does

Four jobs in `.github/workflows/release-cli.yml`, all tag-push-triggered only (never
`workflow_dispatch` — see below):

0. **`build-cbor-macos` / `build-cbor-ios` / `build-cbor-windows`** (each `needs: prepare` only, run in
   parallel with each other and with `publish-pubdev-cbor`'s own toolchain setup): each checks out the
   tag, installs a Rust toolchain targeting that platform (`macos-latest` additionally adds
   `x86_64-apple-darwin` for the macOS job, or all three `*-apple-ios*` triples for the iOS job — the
   runner's host arch is arm64 — via a `rustup target add` RUN step, not the toolchain action's
   `targets:` input, since that input targets `stable` rather than the pinned channel — see either job's
   own comment for the E0463 failure this avoids), installs `just` and the pinned
   `flutter_rust_bridge_codegen`, runs `just cbor-vendor-glue` (required on all three — the generated
   `frb_generated.rs` is gitignored, not committed, so a fresh checkout needs it regenerated before any
   `--features frb-glue` build, on every platform independently), then the platform-specific build (`just
   cbor-vendor-macos` — lipo + `xcodebuild -create-xcframework`, a versioned bundle — `just
   cbor-vendor-ios` — lipo the simulator arches + `xcodebuild -create-xcframework`, TWO flat/shallow
   bundle slices — or `just cbor-vendor-lib windows-x64`), and uploads the result as a named artifact
   (`cbor-native-macos`, `cbor-native-ios`, `cbor-native-windows-x64`) for the publish job to download.
   None of these three artifact sets can be built on `publish-pubdev-cbor`'s own `ubuntu-latest` host —
   you cannot cross-compile a macOS/iOS `.dylib`/xcframework or an MSVC `.dll` from Linux. The macOS and
   Windows uploads carry a single file (a zip, a `.dll`); the iOS upload carries the unpacked xcframework
   DIRECTORY itself — see "The macOS framework ships zipped; the iOS one does not" below for why that
   asymmetry is correct, not an oversight.
1. **`publish-pubdev-cbor`** (`needs: [prepare, build-cbor-macos, build-cbor-ios, build-cbor-windows]`)
   installs its own toolchain: Rust (`dtolnay/rust-toolchain@stable`), `just`/`wasm-pack`
   (`taiki-e/install-action`), Flutter (`subosito/flutter-action` — **not** `dart-lang/setup-dart`, for
   the same reason `ci.yml`'s `cratestack-cbor-*` jobs use Flutter: this package's
   `flutter.plugin.platforms` pubspec key obliges an `environment.flutter` constraint a standalone Dart
   SDK can't satisfy), `flutter_rust_bridge_codegen` pinned `=2.12.0`, pinned `binaryen` (`wasm-opt`,
   avoids an unpinned mid-build download that has failed a real release before — see `release-cli.yml`'s
   `publish-npm-cbor-web` job for the identical incident), `cargo-ndk` pinned `=4.1.2`, the three Android
   rustup targets, and resolves an installed Android NDK (prefers `28.2.13676358`, matching Flutter's own
   default; falls back to the runner's newest preinstalled NDK; fails loudly with directory listings if
   none exists — never silently).
2. **Vendors the Linux/Android/web artifact sets inline, unconditionally, every run:** `just
   cbor-vendor-native`, `cbor-vendor-web`, `cbor-vendor-android`. No "already vendored, skip" shortcut —
   a fresh tag checkout never has them, by design (they're gitignored). Then **downloads** the macOS
   xcframework, iOS xcframework, and Windows `.dll` the three jobs above built, via
   `actions/download-artifact@v4` with an explicit `name:` per artifact (not `pattern:` — there are only
   three, each with a known 1:1 destination directory, so no per-artifact-name subfolder juggling is
   needed). The iOS download's `path:` is the xcframework directory itself, not a parent — the artifact
   IS that directory's contents (see the job-0 bullet above), so this restores it in place rather than
   unpacking anything.
3. **Verifies the archive**, described in detail below. This is a hard gate: the job exits non-zero
   and never reaches the publish step if any artifact is missing from what `dart pub publish
   --dry-run` reports it would ship.
4. Publishes: `dart pub publish --force`, authenticated via pub.dev's GitHub Actions OIDC
   (`id-token: write`, job-scoped — same permission npm's Trusted Publishing uses for its own OIDC
   exchange). `--force` skips the interactive confirmation prompt (required for CI — the command
   would otherwise hang waiting on stdin); it does **not** skip real validation errors, only the
   confirm-to-proceed step for a package pub already considers publishable.

Same tag-push-only gate as every npm job in this workflow (`if: github.event_name == 'push'`): a
`workflow_dispatch` re-run only rebuilds/re-attaches GitHub Release binaries, never touches crates.io,
npm, or pub.dev — publishing to any of the three from a manually-dispatched run would let a throwaway
test tag reach a registry that cannot delete what it published.

## The verification gate, and why it must inspect archive *content*, not exit code

`dart pub publish --dry-run` **exits non-zero (65) even for a fully-vendored, genuinely publishable
package**, because `cratestack_cbor` pins `flutter_rust_bridge` to an exact version
(`flutter_rust_bridge: 2.12.0`, no `^`) rather than a range — required, not a defect: the vendored frb
glue is codegen-version-specific (see the development doc's gotcha 3). Pub's own validator flags this
as "potential issue" #1 every single run, vendored or not:

```
Package validation found the following potential issue:
* Your dependency on "flutter_rust_bridge" should allow more than one version. For example:

  dependencies:
    flutter_rust_bridge: ^2.12.0
  ...
Package has 1 warning.
```

**So the CI gate cannot use the dry-run's exit code to decide whether vendoring worked** — a
correctly-vendored package and a completely broken one can both exit 65 with "Package has N warnings."
The gate instead **counts the archive listing's own content**: `dart pub publish --dry-run`'s tree
output must contain exactly 4 occurrences of `libcratestack_client_flutter.so` (linux-x64 +
arm64-v8a + x86_64 + armeabi-v7a), exactly 2 of `cratestack_cbor_wasm` (`.js` + `_bg.wasm`), exactly 1
of `cratestack_client_flutter.dll` (windows-x64), exactly 1 of
`CratestackCborNative.xcframework.zip` **within the `macos` archive entry** (scoped, not a flat
whole-output grep — see below for why scoping is load-bearing), **exactly 0** unpacked
`CratestackCborNative.xcframework` directory entries within that same `macos` scope, exactly 2
`CratestackCborNative.framework` slice entries **within the `ios` archive entry** (device + universal
simulator), and exactly 0 zip entries within that `ios` scope. The macOS negative assertion is the
load-bearing one for that platform: macOS ships as a *zip*, not as the unpacked framework, because
`dart pub publish` dereferences symlinks and a macOS framework stripped of its symlinks fails
`codesign` outright — see "The macOS framework ships zipped; the iOS one does not" below. An earlier
version of this gate counted entries under the unpacked directory and floored at 3, which could never
have caught the problem: dereferencing *adds* entries rather than removing them, so
the broken shape passed a floor by definition. Fewer than any of these
fails the job before the publish step ever runs.

### Proof the archive-verification gate can fail (and that it must exist at all)

Run locally, with a fully-vendored package as the baseline:

```
$ dart pub publish --dry-run
...
├── blobs
│   ├── android
│   │   ├── arm64-v8a/libcratestack_client_flutter.so (904 KB)
│   │   ├── armeabi-v7a/libcratestack_client_flutter.so (492 KB)
│   │   └── x86_64/libcratestack_client_flutter.so (946 KB)
│   └── linux-x64/libcratestack_client_flutter.so (903 KB)
...
│       └── web/wasm-pkg/cratestack_cbor_wasm.js (21 KB)
│           cratestack_cbor_wasm_bg.wasm (121 KB)
...
Total compressed archive size: 1 MB.
Package has 1 warning.
```

Now delete only `blobs/` (simulating a `cbor-vendor-native`/`cbor-vendor-android` step that silently
failed or was skipped) and run the identical command again:

```
$ rm -rf blobs && dart pub publish --dry-run
...
Total compressed archive size: 131 KB.
Validating package...
Package validation found the following potential issue:
* Your dependency on "flutter_rust_bridge" should allow more than one version. ...
Package has 1 warning.
```

**This is the decisive result.** With the entire native-library payload missing — the actual reason
this package exists — `dart pub publish --dry-run` reports **the exact same single warning** as the
fully-vendored run above. Nothing about the missing `.so` files appears anywhere in the output. Pub's
own validator has no way to notice: the native library is loaded via a hardcoded runtime path string
(`DynamicLibrary.open(...)`/an executable-relative bundle path), never declared anywhere pub's
`dart analyze`-backed checker looks. The archive silently shrinks from 1 MB to 131 KB and pub is
satisfied either way. This is exactly the trap `npm-publishing.md` documents as *"Step 3 is the one
that bites: `npm publish` ships an empty package in silence"* — same failure shape, different
registry — and it is why the CI job's archive-content check (counting `.so`/`wasm` occurrences) is a
hard gate rather than an assumption that the vendor steps ran.

(For completeness: deleting the generated Dart glue too — a truly untouched fresh clone, i.e. also
removing `lib/src/native/rust/` — *does* produce real `dart analyze` errors, because
`native_cbor_codec.dart` then has unresolvable imports. But even those are reported as part of the
same non-fatal "Package has N warnings" bucket, not a hard abort — so relying on dry-run's own exit
status or its "errors vs. warnings" framing is not a safe gate in either failure mode. Counting the
archive's actual file listing is the only check that distinguishes a real payload from a missing one.)

### A second dead check, found (and fixed) while adding iOS

Adding the analogous iOS assertions to this gate (cratestack#563's iOS slice) surfaced a genuine,
pre-existing bug in the macOS negative check, not something this slice introduced: `macos_raw_count`
(the assertion that **0** entries exist for the unpacked `CratestackCborNative.xcframework` directory)
had used a flat, whole-archive `grep -c 'CratestackCborNative\.xcframework/'` (parent name immediately
followed by a `/`) since it was written.

**That pattern can never match anything against this repo's currently-resolved `dart pub` version.**
`dart pub publish --dry-run`'s tree renderer gives every nesting level its own line with a box-drawing
prefix — it never concatenates a parent directory and a child path segment onto one line — so the
substring "a directory name immediately followed by `/`" does not occur anywhere in real output,
leaked or not. Verified directly, not inferred: emptied `.pubignore` locally (simulating the exact bug
this check exists to catch), ran a real `dart pub publish --dry-run`, and confirmed the unpacked
`macos/Frameworks/CratestackCborNative.xcframework/Versions/A/...` tree really was present in the
archive listing — while the old grep pattern still reported 0 against that exact output. Restoring
`.pubignore` and re-running showed the healthy case also reporting 0, which is precisely why the bug
went unnoticed: **both the healthy and the broken shape produced the identical "0" result**, the same
"check that cannot fail" shape as the blobs-deletion experiment above, just in the negative-assertion
half instead of the positive one.

The fix scopes the check to the lines belonging to the `macos` top-level archive entry specifically
(an `awk` capture between that entry and the next top-level sibling), then matches an anchored, bare
`CratestackCborNative.xcframework` directory-header line within that scope — confirmed to report 0
against the healthy fixture and a real nonzero count against the same constructed leaked fixture. The
scoping is required, not cosmetic: `CratestackCborNative.xcframework`/`CratestackCborNative.framework`
can legitimately appear as an unscoped substring under the **iOS** archive entry too (iOS ships its
xcframework unpacked by design — see "The macOS framework ships zipped; the iOS one does not" below —
so a global, unscoped positive count for `.framework` would double-count real iOS entries as if they
were a macOS leak, and an unscoped `.xcframework.zip` count would silently mask a stray, wrongly-added
iOS zip behind the legitimate macOS one). Both the macOS and iOS checks in the current
`release-cli.yml` use this same `awk`-scoped shape now, cross-checked against constructed fixtures for
every failure mode each assertion is meant to catch (missing/extra `.so`, missing iOS slice, missing
iOS entirely, a stray iOS zip, a duplicated macOS zip) before landing — see that job's own comments for
the exact patterns.

## Verify the tarball before publishing anything (manual bootstrap, and every dry run)

Same discipline `npm-publishing.md` establishes for npm's `npm pack --dry-run` — read the archive
listing, don't just check the exit code:

```bash
cd dart-packages/cratestack_cbor
just cbor-vendor-native
just cbor-vendor-web
just cbor-vendor-android
dart pub publish --dry-run
```

The listing must show all three vendored artifact sets with plausible sizes (native libraries in the
hundreds of KB per platform, the wasm pair in the tens/low hundreds of KB) and a total archive size in
the low single-digit megabytes. A total under a few hundred KB with the `blobs/`/`wasm-pkg/` entries
absent from the tree means vendoring didn't run (or ran against the wrong directory) — **do not
publish**, same instruction the development doc's own dry-run section gives.

### `--dry-run` is necessary but not sufficient: pub.dev validates again server-side

A clean `--dry-run` does **not** mean the upload will be accepted. pub.dev runs its own validators
after the archive is uploaded, and rejects there with a different set of rules. This is not
hypothetical — the real first-publish attempt of `cratestack_cbor` 0.8.0 passed `--dry-run` locally,
authorized, uploaded, and was then refused:

```
Uploading... (3.6s)
Message from server: pubspec.yaml allows Flutter SDK version prior to 1.20.0, which does not
support having no `ios/` folder. Please consider increasing the Flutter SDK requirement to
^1.20.0 or higher (environment.sdk.flutter) or create an `ios/` folder.
```

The cause was `environment.flutter: ">=1.10.0"` — enough for pub's *local* check of
`plugin.platforms`, but at 0.8.0 this package shipped no `ios/` folder, and Flutter only permits
omitting platform folders from 1.20 onward. Fixed by raising the constraint (see the pubspec's own
comment for why the number looks low next to `sdk: ^3.5.0`).

**That specific rejection is void as of 0.8.7** — cratestack#563's iOS slice added a real `ios/`
folder and an `ios: ffiPlugin: true` platform entry, so there is no longer an omitted platform
folder for pub.dev to object to. The `>=1.20.0` floor stays anyway on independent grounds, spelled
out in the pubspec's own comment. The episode is kept here because the *lesson* — pub.dev validates
again server-side, with rules `--dry-run` does not run — outlived its trigger; do not read it as a
current description of the package's platform matrix.

Two things worth carrying forward:

- **A server-side rejection is not a partial publish.** Nothing was created; `pub.dev/api/packages/
  cratestack_cbor` still returned 404 afterwards, and the same version number could be retried. That
  is the good case — but do check, rather than assume, before changing the version to work around a
  failure.
- **The archive-contents gate in CI cannot catch this class either.** It verifies what is *in* the
  tarball; these are metadata rules enforced after upload. The only way to discover them is to
  attempt a publish, which is precisely why the first one is manual and done by a human reading the
  output.

## Known characteristic, not a defect: the wasm pair ships on every platform

`pubspec.yaml` declares the web wasm pair (`cratestack_cbor_wasm.js`, `cratestack_cbor_wasm_bg.wasm`,
~145 KB combined) under `flutter: assets:`, because that's the mechanism that gets them into a release
`flutter build web` bundle. Flutter has no per-platform asset conditionals, so **every** consumer —
Android, Linux, macOS, iOS, and Windows alike — carries that ~145 KB of wasm it can never execute.
Verified directly inside a built Android APK
(`assets/flutter_assets/packages/cratestack_cbor/.../cratestack_cbor_wasm_bg.wasm` present in
`app-release.apk`). This is a recorded, deliberate maintainer decision pending a package-structure call
(e.g. splitting the web backend into its own package) — **not addressed by this publishing setup**, and
not something to restructure unilaterally here. See cratestack#563's issue thread (slice 4) for the
full record.

## Generator seam: still blocked on publication

`crates/cratestack-client-dart/templates/pubspec.yaml.j2` still emits `cbor: ^6.5.1` (the pure-Dart
`package:cbor`) into every generated client, and **must**, until `cratestack_cbor` is actually live on
pub.dev. The maintainer decision (cratestack#563, 2026-08-16) is that the generator will switch to
`cratestack_cbor` as a hard replacement, not an opt-in flag — but flipping the template before the
package exists on pub.dev would make every generated client's `dart pub get` fail with "package not
found," including the committed `examples/flutter-riverpod/client` and its own CI drift check. That
flip is explicitly **out of scope** for this publishing slice; it is the next piece of cratestack#563,
sequenced strictly after the first real publish.

## What was not verified here (be honest about this before relying on it)

- **The actual OIDC exchange against pub.dev** — this repository has no live "Automated publishing"
  configuration yet (blocked on the manual first-publish step above), so `publish-pubdev-cbor` has
  never actually run against pub.dev's real servers. Everything else (toolchain install, all three
  vendor recipes, the archive-verification gate, `dart pub publish --dry-run`'s exact output shape) was
  verified by hand against this package as it exists today. The one thing that cannot be verified
  before a real tag push is whether pub.dev's OIDC trust check itself behaves as documented once
  configured — this matches the same category of unverified step `npm-publishing.md` flags for a
  brand-new npm Trusted Publisher entry before its first real CI-triggered publish.
- **The macOS framework ships zipped; the iOS one does not, and neither is a packaging preference.**
  `dart pub publish` dereferences symlinks when it builds its archive. A macOS framework is a
  *versioned* bundle whose three symlinks (`Versions/Current`, the top-level binary, `Resources`) are
  structural, so the dereferenced result is not merely untidy — it is invalid. Measured end to end on a
  real Mac: `dart pub publish --dry-run` lists the binary three times; `codesign` on that shape fails
  with `bundle format is ambiguous (could be app or framework)` while the symlinked original signs and
  verifies cleanly; and a real `flutter build macos` against it dies with
  `Command CodeSign failed with a nonzero exit code`. No symlink-free layout escapes this — the
  alternatives were tried and each fails somewhere else (`unsealed contents present in the root
  directory`, `did not contain an Info.plist`, `does not use shallow bundles`). So `just
  cbor-vendor-macos` also emits `CratestackCborNative.xcframework.zip` (zips store symlinks), the
  package's `.pubignore` keeps the unpacked directory out of the archive, and the podspec's
  `prepare_command` unpacks it at `pod install` time. `just cbor-example-verify-macos` deletes the
  unpacked directory before building so every CI run proves that reconstruction path rather than the
  directory that only ever exists on a build machine — the original defect passed a fully green CI run
  precisely because nothing tested the published shape.

  iOS's xcframework has the OPPOSITE shape and ships the OPPOSITE way, deliberately: iOS frameworks are
  flat/shallow bundles (Apple's own term — no `Versions/` indirection, no symlinks anywhere), and `just
  cbor-vendor-ios` constructs that layout directly with no `ln -s` step at all. With no symlinks to lose,
  `dart pub publish`'s dereferencing behavior has nothing to corrupt, so the unpacked directory ships
  as-is — no zip, no `prepare_command`, no `.pubignore` entry. This is a reasoned conclusion backed by a
  build-time assertion (`just cbor-vendor-ios` counts symlinks in the assembled xcframework and fails
  loudly if the count is ever nonzero — see that recipe's own header comment), not a measurement on real
  hardware: this repo's dev toolchain has no Xcode, so the recipe's first real execution is
  `cratestack-cbor-ios`/`build-cbor-ios` in CI, on `macos-latest`. Verified with `dart pub publish
  --dry-run` against a hand-built fixture matching the intended shape (two flat `.framework` slices under
  one `.xcframework`, cratestack#563's iOS slice): the archive lists both slices' binaries and
  `Info.plist` files directly, with no zip anywhere under `ios/`.

- **Linux arm64** is not vendored by this job and is not claimed by the package — nothing to verify
  here; see "Platform status" above.
- **The macOS, iOS, and Windows legs (`build-cbor-macos`, `build-cbor-ios`, `build-cbor-windows`) have
  not run on a real `macos-latest`/`macos-latest`/`windows-latest` GitHub-hosted runner as part of THIS
  release job** — their steps were written against the landed `just cbor-vendor-macos`/`cbor-vendor-ios`/
  `cbor-vendor-lib windows-x64` recipes and `ci.yml`'s equivalent jobs; the macOS and Windows ones do run
  green on real runners already, but the iOS one (`cratestack-cbor-ios` in `ci.yml`) has never run
  anywhere yet — this release job's `build-cbor-ios` is its first real execution in either workflow. The
  first real test of the *release* legs specifically (as opposed to `ci.yml`'s jobs) is this workflow
  running on a tag push. The macOS symlink question that used to sit here is no longer open — it was
  measured and fixed; see the previous bullet.
- **`pana`** (pub.dev's own scoring tool), if run, scores the package as it exists in this branch, not
  as pub.dev will score it after the package is live with real download/dependency history — some
  pana checks (e.g. popularity-derived signals) only stabilize post-publish.

## The two builder packages (`cratestack_annotations`, `cratestack_builder`)

Added by cratestack#668 phase 1, published manually at 0.8.5 on 2026-08-21, and handled from the
next tag onward by `publish-pubdev-annotations` / `publish-pubdev-builder` in `release-cli.yml`.

Everything above about OIDC, the tag-push-only gate, `setup-dart` adjacency, and the
exits-0-on-authentication-failure verification gate applies to them unchanged — those jobs are
deliberate copies of `publish-pubdev-cbor`'s shape, not a simplified variant. Two differences, both
because these are **pure Dart** packages:

- No `environment.flutter`, so `dart-lang/setup-dart` is the entire toolchain and `dart pub publish`
  runs directly. There is no `FLUTTER_DART` indirection and no cratestack#633 version-solving trap.
- Nothing is vendored, so there is no archive-*content* gate — the `cratestack_cbor` job's
  "does the tarball actually contain the `.so`/wasm" check has no analogue. The version-served gate
  still applies and is still the one that matters.

**They are sequenced, not parallel.** `publish-pubdev-builder` declares
`needs: [prepare, publish-pubdev-annotations]`. Today the order is not load-bearing —
`cratestack_builder` depends on `cratestack_annotations: ^0.8.5`, already published, so it resolves
either way — but on a release where the builder requires a version of the annotation package that
the same run is publishing, parallel jobs would race and the builder's publish would fail version
solving.

### Outstanding: Automated publishing is not yet enabled on either package

Same bootstrap constraint described in "First publish" above: pub.dev can only automate publishing
for a package that already exists, so both were first published by hand. Until a maintainer enables
**Admin → Automated publishing** on each package (repository `cratestack/cratestack`, tag pattern
`v{{version}}`), both jobs will fail on every tag push exactly as `publish-pubdev-cbor` did before
its own setup was completed.

### The inter-package constraint is deliberately not touched by `just bump`

**Corrected 2026-08-23.** An earlier revision of this section called this a latent bug and warned it
would "become wrong at the first minor bump". That was wrong; the behaviour is correct and should be
left alone.

`cratestack_builder` declares `cratestack_annotations: ^0.8.8`, and `just bump`'s rewrite is anchored
to a line starting `version:`, so this indented dependency line is never rewritten. Two reasons that
is right:

**Caret already spans the whole 0.8.x series.** For `0.x` versions pub's caret pins the *second*
component, not the third — `^0.8.5` means `>=0.8.5 <0.9.0`, not `>=0.8.5 <0.8.6`. Verified rather
than assumed: a probe package constraining `^0.8.5` against a registry holding 0.8.5/0.8.6/0.8.7
resolves to **0.8.7**. So the constraint does not need touching as patch versions land.

**The lower bound states an API requirement, not a version relationship.** It should name the
earliest version whose annotation surface the builder actually uses — `^0.8.8` because
`touchFlagFields` and `nonDefaultingListFields` were added there. Bumping it in lockstep would
destroy that meaning; leaving it at `any` or an older floor would let a consumer resolve an
annotation package missing a field the generator emits, failing at codegen with
`undefined_named_parameter` (observed, and the reason 0.8.8 exists at all).

The rule, therefore:

> Raise this constraint **only** when the builder begins using a newly-added annotation field.
> Never as part of a routine version bump.

Which leaves nothing for `just bump` to do here, and no reason to teach it otherwise.

## See also

- [`docs/tooling/cratestack-cbor-development.md`](cratestack-cbor-development.md) — toolchain pins, the
  vendor recipes this document's CI job runs, and the four development-time failure modes.
- [`docs/tooling/npm-publishing.md`](npm-publishing.md) — the npm/crates.io pipeline this sits
  alongside, and the source of the "verify the tarball before publishing" discipline this document
  applies to a second registry.
- [`RELEASE.md`](../../RELEASE.md) — the end-to-end release process this job is one part of.
