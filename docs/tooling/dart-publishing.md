# pub.dev publishing setup (`cratestack_cbor`)

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
gotcha 1). **This means any publish workflow MUST vendor all three artifact sets itself, immediately
before publishing, in the same job** — there is no committed fallback, and pub.dev gives **no signal**
if a vendoring step is skipped or fails partway (see "Proof the archive-verification gate can fail"
below). `publish-pubdev-cbor` in `release-cli.yml` does exactly this: install toolchain → vendor all
three → verify the archive → publish, in one job, every release, unconditionally.

## Platform status at time of writing

Linux x86_64, Android (arm64-v8a, x86_64, armeabi-v7a), and web. iOS, macOS, Windows, and Linux arm64
are **not** in the vendored archive — every other platform throws `UnsupportedError` at runtime. This
mirrors what the package's own `pubspec.yaml` and README already say; it is not something this
publishing setup changes or should change (iOS/macOS/Windows are a deliberate maintainer hold, not
in scope here — see the ticket).

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

`publish-pubdev-cbor` (`.github/workflows/release-cli.yml`), tag-push-triggered only (never
`workflow_dispatch` — see below):

1. Installs the full toolchain: Rust (`dtolnay/rust-toolchain@stable`), `just`/`wasm-pack`
   (`taiki-e/install-action`), Flutter (`subosito/flutter-action` — **not** `dart-lang/setup-dart`,
   for the same reason `ci.yml`'s three `cratestack-cbor-*` jobs use Flutter: this package's
   `flutter.plugin.platforms` pubspec key obliges an `environment.flutter` constraint a standalone
   Dart SDK can't satisfy), `flutter_rust_bridge_codegen` pinned `=2.12.0`, pinned `binaryen`
   (`wasm-opt`, avoids an unpinned mid-build download that has failed a real release before — see
   `release-cli.yml`'s `publish-npm-cbor-web` job for the identical incident), `cargo-ndk` pinned
   `=4.1.2`, the three Android rustup targets, and resolves an installed Android NDK (prefers
   `28.2.13676358`, matching Flutter's own default; falls back to the runner's newest preinstalled
   NDK; fails loudly with directory listings if none exists — never silently).
2. **Vendors all three artifact sets, unconditionally, every run:** `just cbor-vendor-native`,
   `cbor-vendor-web`, `cbor-vendor-android`. No "already vendored, skip" shortcut — a fresh tag
   checkout never has them, by design (they're gitignored).
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
arm64-v8a + x86_64 + armeabi-v7a) and exactly 2 of `cratestack_cbor_wasm` (`.js` + `_bg.wasm`). Fewer
than that fails the job before the publish step ever runs.

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

## Known characteristic, not a defect: the wasm pair ships on every platform

`pubspec.yaml` declares the web wasm pair (`cratestack_cbor_wasm.js`, `cratestack_cbor_wasm_bg.wasm`,
~145 KB combined) under `flutter: assets:`, because that's the mechanism that gets them into a release
`flutter build web` bundle. Flutter has no per-platform asset conditionals, so **every** consumer —
Android, Linux, and eventually iOS/macOS/Windows once those land — carries that ~145 KB of wasm it can
never execute. Verified directly inside a built Android APK
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
- **iOS, macOS, Windows, Linux arm64** are not vendored by this job and are not claimed by the
  package — nothing to verify here; see "Platform status" above.
- **`pana`** (pub.dev's own scoring tool), if run, scores the package as it exists in this branch, not
  as pub.dev will score it after the package is live with real download/dependency history — some
  pana checks (e.g. popularity-derived signals) only stabilize post-publish.

## See also

- [`docs/tooling/cratestack-cbor-development.md`](cratestack-cbor-development.md) — toolchain pins, the
  vendor recipes this document's CI job runs, and the four development-time failure modes.
- [`docs/tooling/npm-publishing.md`](npm-publishing.md) — the npm/crates.io pipeline this sits
  alongside, and the source of the "verify the tarball before publishing" discipline this document
  applies to a second registry.
- [`RELEASE.md`](../../RELEASE.md) — the end-to-end release process this job is one part of.
