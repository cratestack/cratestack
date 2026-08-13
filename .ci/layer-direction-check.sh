#!/usr/bin/env bash
# Layer-direction check (ADR 0014, docs/adr/0014-layer-direction-enforcement.md).
#
# Asserts `dep.layer <= self.layer` for every NORMAL cratestack-* -> cratestack-*
# dependency edge among the crates under `crates/`, against the assignment in
# `docs/adr/layers.toml`. Catches the #465 defect class mechanically: a
# storage/binding/facade crate quietly depending "up" the stack because a
# trait or const happened to be defined in the wrong crate.
#
# Scope, matching ADR 0014's Decision:
#   - Normal dependencies only (`kind` is null in `cargo metadata`),
#     INCLUDING target-gated ones (e.g. cratestack-sqlite's
#     `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` edge to
#     cratestack-client-rust) — cargo reports those with `kind: null` and a
#     non-null `target`, so filtering on `kind` alone already includes them;
#     no separate branch is needed.
#   - Dev-dependencies (`kind: "dev"`) are exempt. Established precedent:
#     cratestack-redis dev-depends on cratestack-axum post-#465 for `Layer`
#     types in its own tests, and that is correct — a dev-only edge never
#     ships in the compiled artifact and creates no link-time coupling.
#   - Build-dependencies (`kind: "build"`) are exempt for the same reason: a
#     build-dependency compiles and runs at the depending crate's OWN build
#     time (inside its build.rs) and is never part of a downstream
#     consumer's resolved graph or the crate's own linked output — the same
#     "doesn't ship" argument that exempts dev-dependencies. At the time of
#     writing the only build-dependency edges anywhere in the workspace are
#     cratestack-studio -> {flate2, tar} (external crates, not cratestack-*),
#     so this choice changes no currently-verified pass/fail outcome; it is
#     recorded here so a future cratestack-* build-dependency edge doesn't
#     have to reopen the question.
#   - An unassigned crate under `crates/` — one `cargo metadata` reports that
#     `docs/adr/layers.toml` has no entry for, under any of its three tables
#     ([layers]/[tools]/[vitrine]) — is a hard failure, not a skip. A
#     checker that silently ignores an unknown crate gives no signal on the
#     PR that most needs one: the PR adding the crate.
#   - Violations naming an edge listed in
#     `.ci/layer-direction-allowlist.toml` are reported but do not fail the
#     job. An allowlist entry that does NOT match any actual violation
#     (edge fixed, or edge no longer exists) is itself a hard failure — see
#     that file's header.
#
# Why cargo metadata + Python instead of pure cargo-tree text parsing (the
# `.ci/feature-matrix.sh` shape) or pure jq (the shape ADR 0014's Context
# section gestures at): the input here is two TOML files (the layer
# manifest and the allowlist) joined against `cargo metadata`'s JSON, and
# jq has no TOML reader. `justfile`'s own `_fmt` recipe and the
# `release-publish` recipe already reach for `python3 -c "import json,..."`
# to process `cargo metadata --format-version=1` output for exactly this
# "build a small graph, do a bit of set logic" shape — this script follows
# that precedent (stdlib `tomllib`, Python 3.11+, requires no extra
# dependency) rather than the cargo-tree-grep shape, which doesn't apply
# here since nothing needs to be compiled.
#
# `cargo metadata --no-deps` is manifest-only — no crate is compiled, no
# feature is resolved, and it costs nothing extra in CI: the `check` job in
# `.github/workflows/ci.yml` already runs `cargo metadata --locked` as its
# very first step, before `rust-cache`.
#
# Run locally via `just verify-layering`. CI runs it as the
# `layer-direction` job in `.github/workflows/ci.yml`.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

LAYERS_TOML="docs/adr/layers.toml"
ALLOWLIST_TOML="${LAYER_CHECK_ALLOWLIST:-.ci/layer-direction-allowlist.toml}"

if ! command -v python3 >/dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

cargo metadata --no-deps --format-version=1 | python3 "$(dirname "${BASH_SOURCE[0]}")/layer_direction_check.py" \
  --layers "$LAYERS_TOML" \
  --allowlist "$ALLOWLIST_TOML"
