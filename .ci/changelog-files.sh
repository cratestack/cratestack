#!/usr/bin/env bash
# Single declared source of truth for every changelog the release pipeline
# seeds and checks. `.ci/changelog-seed.sh` and `.ci/changelog-check.sh` both
# source this file for their default file set instead of each hardcoding its
# own path — a newly-added publishable package that ships a CHANGELOG.md
# gets added here, once, so its omission (forgetting to add it) is visible
# by inspection rather than a path quietly missing from one script but not
# the other.
#
# Paths are relative to the repository root. This file only declares data —
# it is meant to be `source`d, not executed directly.

CHANGELOG_FILES_DEFAULT=(
  "CHANGELOG.md"
  "dart-packages/cratestack_cbor/CHANGELOG.md"
  "dart-packages/cratestack_annotations/CHANGELOG.md"
  "dart-packages/cratestack_builder/CHANGELOG.md"
)

# Packages eligible for changelog-seed.sh's "no-op auto-fill" fallback
# (cratestack#713). When a file above is about to fall back to the
# marker+commit-list placeholder (nothing under its own "## Unreleased" to
# carry forward — see changelog-seed.sh's per-file branches), and the file
# is a key here, changelog-seed.sh first checks the declared scope below for
# non-bump commits since the last release tag. Zero commits anywhere in
# scope writes the repo's existing, stable "No functional changes..."
# wording directly — no TODO marker, no manual edit needed, the
# `changelog (no unedited seeds)` gate passes. One or more commits anywhere
# in scope still writes the placeholder, unchanged — this does not weaken
# the gate for a package that genuinely changed.
#
# The scope for a package is deliberately NOT just its own directory.
# cratestack#713 established that "dart-packages/cratestack_cbor/" ALONE is
# an unsafe proxy for "no functional change": that package vendors prebuilt
# binaries (native .so/.dylib/.dll, xcframework, jniLibs, wasm — see
# justfile's `cbor-vendor-*` recipes) built at release time from Rust crates
# that live OUTSIDE dart-packages/cratestack_cbor/ and are never committed.
# Concretely: `cbor-vendor-lib` runs `cargo build -p
# cratestack-client-flutter --features frb-glue`, and `cbor-vendor-web` runs
# `wasm-pack` over crates/cratestack-cbor-wasm. A source change in either —
# or in what either COMPILES IN — can change the shipped bytes while the
# package's own directory stays untouched.
#
# crates/cratestack-codec-cbor is in scope because of the DECODE path
# specifically, and that is worth naming, because the encode path is NOT it.
# Both vendoring sources call the codec's decoder bare, nothing in between:
#
#   crates/cratestack-client-flutter/src/cbor/mod.rs   CborCodec.decode(bytes)
#   crates/cratestack-cbor-wasm/src/wasm.rs            CborCodec.decode(bytes)
#
# So any decode-side change in the codec reaches both vendored binaries
# directly. Their ENCODE paths both wrap the value first (`EncodableValue`,
# which routes `Value::Null` through `serialize_none()` at every position in
# the tree), so an encode-side codec change may or may not be observable
# here — it depends on whether that wrapper already covered it.
#
# cratestack#727: v0.8.6 was previously cited here as a load-bearing
# counter-example — a real codec fix (cratestack#675) in `v0.8.5..v0.8.6`
# with zero commits under the package directory. The git facts are right;
# the conclusion drawn from them was not. That fix was encode-only, and
# `EncodableValue` had already shipped in v0.8.5 (`52b50cea`,
# cratestack#580), so the wrapper bypassed the fixed branch entirely and the
# vendored bytes did not change. #675's own commit message says so: the
# wrappers are "kept as intentional defense-in-depth ... not as the only
# thing preventing the bug". Its edits to crates/cratestack-cbor-wasm in
# that same range were module-doc and test-only. v0.8.6 was a genuine no-op
# for this package, and the "No functional changes" wording now on `main`
# for that section is CORRECT — do not "fix" it.
#
# That makes v0.8.6 an example of this scope firing CONSERVATIVELY (in-scope
# commits, unchanged bytes) — the safe direction, costing at worst a hand-
# written changelog line for a release that did not need one. It is not
# evidence that the directory-only proxy has already failed in production.
# The justification for the widened scope is the decode path above: a
# mechanism verified end to end, not a historical incident.
#
# The generalisable trap, recorded so it is not repeated: A DEPENDENCY EDGE
# IS NOT A BEHAVIOUR PATH. cratestack-client-flutter depends on
# cratestack-codec-cbor — true, trivial to verify, and insufficient on its
# own. Whether a change in a dependency is observable depends on how the
# consumer CALLS it. Checking the edge and stopping there is exactly what
# produced the false claim above.
#
# The problem this fallback does solve is real and evidenced: v0.8.6 shipped
# the RAW, unedited seed placeholder to pub.dev (`git show
# v0.8.6:dart-packages/cratestack_cbor/CHANGELOG.md` — the `<!-- TODO -->`
# marker and the "Do not commit with this placeholder text" line, both
# present), because main has no required status checks and the failing gate
# did not block the merge. With this fallback in place, a release that is a
# genuine no-op across the scope below gets the stable "No functional
# changes" wording written automatically instead.
#
# It deliberately does NOT reach further into shared crates like
# crates/cratestack-core: `git log v0.8.7..v0.8.9 -- crates/cratestack-core`
# shows 2 commits in a range the issue's own table (and this repo's actual
# release) confirms was a genuine no-op for cratestack_cbor — both
# comment/doc-only edits (a doctest-fence conversion, a README typo fix),
# unrelated to what ships. crates/cratestack-core changes on nearly every
# release for reasons that have nothing to do with CBOR encoding, so
# including it here would make this fallback almost never fire, defeating
# the point. If a future release ever proves core-only changes can alter
# cratestack_cbor's shipped bytes without touching cratestack-codec-cbor's
# own directory, that would be a new, narrower gap to close then — not one
# to guess at now.
#
# dart-packages/cratestack_annotations and dart-packages/cratestack_builder
# ARE included, unlike an earlier draft of this file assumed — that draft's
# reasoning ("neither has release history of the pattern yet") was true but
# beside the point: the pattern doesn't need history to exist, only the
# next zero-commit release needs to happen, and skipping them here would
# have silently left them needing the same hand-edit forever (nothing else
# in this file would have caught that — see CHANGELOG_NOOP_EXEMPT and its
# guard in changelog-seed.sh, added for exactly this reason). Checked and
# confirmed (not assumed) before adding them: `release-cli.yml`'s
# `publish-pubdev-annotations`/`publish-pubdev-builder` jobs are PURE
# `dart pub publish` from each package's own checked-out directory — no
# build step, no downloaded artifact, no `environment.flutter`, nothing
# vendored (that job's own comment says so: "PURE DART packages ... no
# vendored native artifacts") — and neither directory contains anything
# resembling a vendored binary (`.so`/`.dll`/`.dylib`/`.wasm`/`blobs`/
# `xcframework`: none found). Unlike cratestack_cbor, their own directory
# IS the complete scope — nothing outside it can change what gets
# published. Both DO have real, non-bump history inside their own
# directory (verified, not assumed a permanent no-op): `git log
# v0.8.7..v0.8.9 -- dart-packages/cratestack_annotations` shows real
# feature work (cratestack#699), and `git log v0.8.9..v0.8.10 --
# dart-packages/cratestack_annotations` shows a pubspec-version correction
# (cratestack#710) — so the inverse (placeholder-still-fires) case matters
# for these two exactly as much as it does for cratestack_cbor.
declare -A CHANGELOG_NOOP_SCOPES=(
  ["dart-packages/cratestack_cbor/CHANGELOG.md"]="dart-packages/cratestack_cbor crates/cratestack-client-flutter crates/cratestack-cbor-wasm crates/cratestack-codec-cbor"
  ["dart-packages/cratestack_annotations/CHANGELOG.md"]="dart-packages/cratestack_annotations"
  ["dart-packages/cratestack_builder/CHANGELOG.md"]="dart-packages/cratestack_builder"
)

# Files in CHANGELOG_FILES_DEFAULT above that are deliberately exempt from
# needing a CHANGELOG_NOOP_SCOPES entry — checked by name, by
# changelog-seed.sh's own guard (see there), against every file actually
# declared. Today this is only the root CHANGELOG.md: it always takes the
# "convert existing '## Unreleased' prose" branch (PRs land narrative prose
# there directly, continuously, as they merge — see CONTRIBUTING.md), never
# the placeholder fallback the no-op mechanism exists to soften, so a scope
# for it would never be consulted. Acceptance criterion (cratestack#713):
# the root changelog path is unaffected by this feature.
CHANGELOG_NOOP_EXEMPT=(
  "CHANGELOG.md"
)
