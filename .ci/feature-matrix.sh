#!/usr/bin/env bash
# Feature-graph regression matrix (cratestack#421).
#
# Two defects that issue reported:
#   1. `decimal-bigdecimal` was a dead `compile_error!` — fixed by removing
#      it as a selectable feature entirely (#421 commit b7f9007).
#   2. Every dependency edge onto `cratestack-core` (and the facade-to-
#      runtime edges cratestack-pg -> cratestack-sqlx,
#      cratestack-sqlite -> cratestack-rusqlite) omitted
#      `default-features = false`, so `cratestack-core`'s
#      `default = ["decimal-rust-decimal"]` was force-enabled even when a
#      consumer explicitly asked for a narrower feature set.
#
# This script runs the exact reproduction commands from the issue and
# fails loudly if either regresses. Run locally via `just feature-matrix`;
# CI runs it as the `feature-matrix` job in `.github/workflows/ci.yml`.
#
# Deliberately NOT a `#[test]` inside a crate: Cargo feature-graph shape
# (what got enabled, and why) isn't observable from inside the compiled
# binary — `cargo tree -e features` inspecting the *resolved* graph from
# outside is the only way to assert on it.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

FAILED=0

fail() {
  echo "::error::$1" >&2
  FAILED=1
}

echo "== [1/4] decimal-bigdecimal must not be a selectable feature (AC1) =="
if OUTPUT=$(cargo check -p cratestack-core --no-default-features --features decimal-bigdecimal 2>&1); then
  fail "expected 'cargo check -p cratestack-core --features decimal-bigdecimal' to be rejected (the feature must not exist), but it succeeded"
  echo "$OUTPUT"
elif ! grep -q "does not contain this feature" <<<"$OUTPUT"; then
  fail "expected an 'unknown feature' error for decimal-bigdecimal, got a different failure instead"
  echo "$OUTPUT"
else
  echo "ok: decimal-bigdecimal is not exposed as a selectable feature"
fi

echo "== [2/4] cratestack-core compiles clean on its own default feature set =="
if ! cargo check -p cratestack-core; then
  fail "cargo check -p cratestack-core (default features) failed"
fi

# Asserts that `cratestack-core`'s own `default` feature set is never part
# of the resolved graph for a given (package, no-default-features, features)
# combination — i.e. nothing downstream leaked it in behind the consumer's
# back. The *specific* features the consumer asked for (decimal-rust-decimal
# included) are expected to still show up individually; only the literal
# `"default"` feature-set node must be absent.
assert_no_default_leak() {
  local pkg="$1"
  shift
  local features="$1"
  shift
  echo "== cargo tree -p $pkg --no-default-features --features $features -e features =="
  local tree
  if ! tree=$(cargo tree -p "$pkg" --no-default-features --features "$features" -e features 2>&1); then
    fail "cargo tree failed for -p $pkg --features $features"
    echo "$tree"
    return
  fi
  if grep -q 'cratestack-core feature "default"' <<<"$tree"; then
    fail "cratestack-core's default feature set leaked into '$pkg --no-default-features --features $features' — a dependency edge is missing default-features = false or an explicit feature forward"
    grep -n "cratestack-core" <<<"$tree" >&2
  else
    echo "ok: no cratestack-core default-feature leak for $pkg --features $features"
  fi
}

echo "== [3/4] cratestack-pg, postgres only: must compile AND must not leak cratestack-core's default features (AC2) =="
if ! cargo check -p cratestack-pg --no-default-features --features postgres; then
  fail "cargo check -p cratestack-pg --no-default-features --features postgres failed to compile"
else
  assert_no_default_leak cratestack-pg postgres
fi

echo "== [4/4] cratestack-sqlite, explicit decimal-rust-decimal only: must compile AND must not leak cratestack-core's default features =="
if ! cargo check -p cratestack-sqlite --no-default-features --features decimal-rust-decimal; then
  fail "cargo check -p cratestack-sqlite --no-default-features --features decimal-rust-decimal failed to compile"
else
  assert_no_default_leak cratestack-sqlite decimal-rust-decimal
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "feature-matrix: FAILED — see ::error:: lines above" >&2
  exit 1
fi

echo "feature-matrix: all checks passed"
