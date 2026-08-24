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
# cratestack#713 proved that "dart-packages/cratestack_cbor/" ALONE is an
# unsafe proxy for "no functional change": that package vendors prebuilt
# binaries (native .so/.dylib/.dll, xcframework, jniLibs, wasm — see
# justfile's `cbor-vendor-*` recipes) built at release time from Rust
# crates that live OUTSIDE dart-packages/cratestack_cbor/ and are never
# committed. v0.8.6 is the load-bearing counter-example, not a hypothetical
# one: `git log v0.8.5..v0.8.6 -- crates/cratestack-codec-cbor` shows a real
# CBOR-encoding bug fix (cratestack#675) landed in that range, in a crate
# depended on directly by BOTH crates/cratestack-client-flutter (the native
# vendoring source — see its Cargo.toml) and crates/cratestack-cbor-wasm
# (the web one — see its Cargo.toml), so the fix was baked into the
# binaries v0.8.6 vendored — while `git log v0.8.5..v0.8.6 --
# dart-packages/cratestack_cbor/` shows ZERO commits, and the
# hand-written v0.8.6 changelog entry claimed "No functional changes"
# anyway. Scoping to the package directory alone would have written that
# same wrong claim automatically instead of a human writing it by mistake —
# worse, not better. The scope below closes that specific, evidenced gap.
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
# Deliberately does NOT include dart-packages/cratestack_annotations or
# dart-packages/cratestack_builder: as of cratestack#713 neither has any
# release history of this pattern (both shipped their first tracked
# release, 0.8.10, with real hand-written prose) — there is nothing to fix
# there yet, and no evidence to size a scope from if there were.
declare -A CHANGELOG_NOOP_SCOPES=(
  ["dart-packages/cratestack_cbor/CHANGELOG.md"]="dart-packages/cratestack_cbor crates/cratestack-client-flutter crates/cratestack-cbor-wasm crates/cratestack-codec-cbor"
)
