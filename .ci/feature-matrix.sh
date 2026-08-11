#!/usr/bin/env bash
# Feature-graph regression matrix (cratestack#421, cratestack#495).
#
# History:
#   1. `decimal-bigdecimal` was originally a dead `compile_error!` — removed
#      as a selectable feature entirely by #421 (commit b7f9007).
#   2. #421 (commit cfde4e0) then found every dependency edge onto
#      `cratestack-core` (and the facade-to-runtime edges cratestack-pg ->
#      cratestack-sqlx, cratestack-sqlite -> cratestack-rusqlite) omitted
#      `default-features = false`, so `cratestack-core`'s
#      `default = ["decimal-rust-decimal"]` was force-enabled even when a
#      consumer explicitly asked for a narrower feature set.
#   3. #495 implements `decimal-bigdecimal` for real: `cratestack-core` now
#      cfg-gates `Decimal` per backend and hard-errors if neither or both
#      are selected. Making that swap actually reachable meant widening
#      #421's "one shared `default-features = false` dependency edge" fix
#      to every crate in the transitive closure between a facade and
#      `cratestack-core` — `cratestack-sql`, `cratestack-policy`,
#      `cratestack-parser`, `cratestack-proto`, `cratestack-macros`,
#      `cratestack-axum`, `cratestack-codec-cbor`, `cratestack-codec-json`
#      all gained the same `default-features = false` + explicit-forward
#      treatment `cratestack-core`/`cratestack-sqlx`/`cratestack-rusqlite`/
#      `cratestack-client-rust` already had — a single crate left pinning
#      `decimal-rust-decimal` anywhere in that closure re-forces it for the
#      whole graph, since Cargo features are additive and unify globally.
#
# This script runs a representative matrix across every crate this PR
# touched (not just the issue's own two repro commands): every facade with
# its own decimal toggle is checked under its default feature set AND both
# narrowed `--no-default-features` selections — `decimal-rust-decimal` and
# the new `decimal-bigdecimal` — asserting no leak either way. Every "plain"
# crate whose `cratestack-core` edge became explicit gets a real compile
# check so a typo'd/missing forward on any one of them fails loudly instead
# of only surfacing when someone happens to build that specific crate. Run
# locally via `just feature-matrix`; CI runs it as the `feature-matrix` job
# in `.github/workflows/ci.yml`.
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

step() {
  echo
  echo "== $1 =="
}

# Asserts that `cratestack-core`'s own `default` feature set is never part
# of the resolved graph for a given (package, no-default-features, features)
# combination — i.e. nothing downstream leaked it in behind the consumer's
# back. The *specific* features the consumer asked for are expected to
# still show up individually; only the literal `"default"` feature-set node
# must be absent.
assert_no_default_leak() {
  local pkg="$1"
  local features="$2"
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

# Asserts that, for a `decimal-bigdecimal`-selecting combination, `rust_decimal`
# never appears anywhere in the resolved graph — this is the acceptance bar
# cratestack#495 cares about: compiling under `decimal-bigdecimal` while
# `rust_decimal` is still reachable somewhere means the swap didn't actually
# happen, even if the build itself is green.
assert_no_rust_decimal() {
  local pkg="$1"
  local features="$2"
  local tree
  if ! tree=$(cargo tree -p "$pkg" --no-default-features --features "$features" -e features 2>&1); then
    fail "cargo tree failed for -p $pkg --features $features"
    echo "$tree"
    return
  fi
  if grep -qi 'rust_decimal' <<<"$tree"; then
    fail "'rust_decimal' is still reachable in '$pkg --no-default-features --features $features' — decimal-bigdecimal did not fully displace it"
    grep -ni "rust_decimal" <<<"$tree" >&2
  else
    echo "ok: no rust_decimal anywhere in the graph for $pkg --features $features"
  fi
}

# Checks a (package, feature-args...) combination compiles, then (when
# --no-default-features is among the args) asserts no default-feature leak.
check_combo() {
  local pkg="$1"
  shift
  echo "-- cargo check -p $pkg $* --"
  if ! cargo check -p "$pkg" "$@"; then
    fail "cargo check -p $pkg $* failed to compile"
    return
  fi
  for arg in "$@"; do
    if [[ "$arg" == --no-default-features ]]; then
      # Find the --features value, if any, among the remaining args.
      local features=""
      local prev=""
      for a in "$@"; do
        if [[ "$prev" == "--features" ]]; then
          features="$a"
        fi
        prev="$a"
      done
      if [[ -n "$features" ]]; then
        assert_no_default_leak "$pkg" "$features"
        if [[ "$features" == *decimal-bigdecimal* ]]; then
          assert_no_rust_decimal "$pkg" "$features"
        fi
      fi
    fi
  done
}

step "[1/6] decimal-rust-decimal and decimal-bigdecimal are mutually exclusive (cratestack#495); neither is NOT an error (cratestack#505)"
if OUTPUT=$(cargo check -p cratestack-core --no-default-features --features decimal-rust-decimal,decimal-bigdecimal 2>&1); then
  fail "expected 'cargo check -p cratestack-core --features decimal-rust-decimal,decimal-bigdecimal' (both backends at once) to be rejected, but it succeeded"
  echo "$OUTPUT"
elif ! grep -q "mutually exclusive" <<<"$OUTPUT"; then
  fail "expected the mutual-exclusion compile_error! for both decimal backends, got a different failure instead"
  echo "$OUTPUT"
else
  echo "ok: selecting both decimal backends at once is rejected"
fi
# cratestack#505: this used to be a hard `compile_error!` too (the "neither"
# arm) — the exact break a consumer legitimately narrowing its graph via
# `default-features = false`, and never touching `Decimal`, hit in the
# wild. `cratestack-core` now compiles cleanly with `Decimal` (and anything
# that references it, e.g. `validate_range_decimal`) simply absent from its
# public surface in this configuration — see `src/decimal.rs`'s module doc
# for why a real `rust_decimal`-backed fallback isn't reachable here (it
# would require `rust_decimal` to stop being a Cargo-optional dependency,
# which breaks the [5/6] no-leak invariant below).
if OUTPUT=$(cargo check -p cratestack-core --no-default-features 2>&1); then
  echo "ok: selecting neither decimal backend compiles cleanly (cratestack#505)"
else
  fail "expected 'cargo check -p cratestack-core --no-default-features' (no backend) to succeed as of cratestack#505, but it failed"
  echo "$OUTPUT"
fi
# `cargo test`, not just `cargo check`: proves the test binary actually
# links and runs in this configuration, not just that `rustc` accepts the
# lib — see `no_decimal_backend_tests` in `src/decimal.rs`.
if OUTPUT=$(cargo test -p cratestack-core --no-default-features 2>&1); then
  if ! grep -q "no_decimal_backend_tests::crate_builds_and_runs_with_no_decimal_backend_selected ... ok" <<<"$OUTPUT"; then
    fail "cargo test -p cratestack-core --no-default-features succeeded, but the cratestack#505 regression test didn't run — check src/decimal.rs's cfg gating"
    echo "$OUTPUT"
  else
    echo "ok: cratestack-core's test suite (including the cratestack#505 regression test) runs with neither decimal backend selected"
  fi
else
  fail "cargo test -p cratestack-core --no-default-features failed"
  echo "$OUTPUT"
fi

step "[2/6] cratestack-core compiles clean on each backend individually"
if ! cargo check -p cratestack-core; then
  fail "cargo check -p cratestack-core (default features) failed"
fi
if ! cargo check -p cratestack-core --no-default-features --features decimal-bigdecimal; then
  fail "cargo check -p cratestack-core --no-default-features --features decimal-bigdecimal failed"
fi

step "[3/6] facade crates: default features AND both narrowed backend selections (AC2/AC4 matrix)"
# (package, extra args...) — every facade that exposes its own decimal
# toggle, checked at its default feature set and at both backends
# individually, so leaks can't hide behind whichever feature set happens to
# be the default, and `decimal-bigdecimal` gets exactly the same coverage
# `decimal-rust-decimal` always has.
check_combo cratestack-pg
# `--features postgres` alone (no explicit decimal choice) is a DELIBERATE
# compile failure as of cratestack#495 — see `cratestack-pg`'s `postgres`
# feature doc comment. Before #495 there was only one decimal backend, so
# `postgres` could safely force it unconditionally; doing that today would
# make `decimal-bigdecimal` unreachable through this facade (both backends
# would end up selected on `cratestack-sqlx` simultaneously). Assert the
# failure explicitly so a future change that "fixes" this by silently
# re-adding the unconditional force gets caught here.
#
# cratestack#505 changed WHICH error this hits, not WHETHER it fails:
# `cratestack-sql`'s `SqlValue::Decimal(cratestack_core::Decimal)` variant
# references `cratestack_core::Decimal` unconditionally (same shape as the
# `validators.rs` code cratestack-core itself now gates — see that crate's
# module doc), and `cratestack-sql` doesn't get the same treatment here
# (out of this PR's scope — this facade's `postgres` path structurally
# needs a real decimal backend regardless, same as before #505). So
# `Decimal` not existing on `cratestack-core` in this configuration now
# surfaces as a plain rustc "cannot find type `Decimal`" from deep inside
# `cratestack-sql`, instead of `cratestack-core`'s old, clearer
# "enable exactly one decimal backend" `compile_error!` pointing at the
# actual missing choice. Still a hard, loud failure — never a silent
# success — just a worse diagnostic; asserting on it here catches any
# future change that turns this back into a silent backend force.
echo "-- cargo check -p cratestack-pg --no-default-features --features postgres (expected to fail) --"
if OUTPUT=$(cargo check -p cratestack-pg --no-default-features --features postgres 2>&1); then
  fail "expected 'cargo check -p cratestack-pg --no-default-features --features postgres' (no decimal backend) to fail, but it succeeded — postgres may be silently forcing a backend again"
  echo "$OUTPUT"
elif ! grep -q "cannot find type \`Decimal\` in crate \`cratestack_core\`" <<<"$OUTPUT"; then
  fail "cargo check -p cratestack-pg --no-default-features --features postgres failed, but not with the expected 'cannot find type Decimal' error (cratestack#505 changed this message — see the comment above)"
  echo "$OUTPUT"
else
  echo "ok: postgres alone (no decimal choice) fails as expected"
fi
check_combo cratestack-pg --no-default-features --features postgres,decimal-rust-decimal
check_combo cratestack-pg --no-default-features --features postgres,decimal-bigdecimal
check_combo cratestack-sqlite
check_combo cratestack-sqlite --no-default-features --features decimal-rust-decimal
check_combo cratestack-sqlite --no-default-features --features decimal-bigdecimal
check_combo cratestack-sql
check_combo cratestack-sql --no-default-features --features decimal-rust-decimal
check_combo cratestack-sql --no-default-features --features decimal-bigdecimal
check_combo cratestack-sqlx
check_combo cratestack-sqlx --no-default-features --features decimal-rust-decimal
check_combo cratestack-sqlx --no-default-features --features decimal-bigdecimal
check_combo cratestack-rusqlite
check_combo cratestack-rusqlite --no-default-features --features decimal-rust-decimal
check_combo cratestack-rusqlite --no-default-features --features decimal-bigdecimal
check_combo cratestack-api
check_combo cratestack-api --no-default-features --features decimal-rust-decimal
check_combo cratestack-api --no-default-features --features decimal-bigdecimal
check_combo cratestack-client
check_combo cratestack-client --no-default-features --features decimal-rust-decimal
check_combo cratestack-client --no-default-features --features decimal-bigdecimal
check_combo cratestack-cli
# cratestack#496: `cratestack-cli`'s decimal toggle previously only forwarded
# to 5 of its 9 real `cratestack-core`-consuming dependencies —
# `cratestack-studio`, `cratestack-mock-wiremock`, `cratestack-client-dart`,
# and `cratestack-client-typescript` were plain `.workspace = true` edges
# with no `default-features = false`, so their own default backend stayed
# force-enabled regardless of what this crate requested. That made
# `--no-default-features --features decimal-bigdecimal` a hard
# `compile_error!` that pointed nowhere near the real cause. Same coverage
# every other facade in this step already gets, now that the forward is
# complete.
check_combo cratestack-cli --no-default-features --features decimal-rust-decimal
check_combo cratestack-cli --no-default-features --features decimal-bigdecimal

step "[4/6] plain crates: every cratestack-core edge this PR made explicit actually compiles"
# These ~22 crates had no decimal toggle of their own before cratestack#421 —
# they simply need cratestack-core's default = false + an explicit
# features = ["decimal-rust-decimal"] forward to keep compiling at all now
# that the leak is closed. A default `cargo check` on each one is enough to
# catch a missing/typo'd forward; there's no narrower feature set to select
# here (that's the "plain" in plain crate).
for pkg in \
  cratestack-axum \
  cratestack-cbor-napi \
  cratestack-client-dart \
  cratestack-client-flutter \
  cratestack-client-rust \
  cratestack-client-store-sqlite \
  cratestack-client-typescript \
  cratestack-codec-cbor \
  cratestack-codec-json \
  cratestack-grpc \
  cratestack-lsp \
  cratestack-macros \
  cratestack-migrate \
  cratestack-mock-wiremock \
  cratestack-parser \
  cratestack-policy \
  cratestack-proto \
  cratestack-redis \
  cratestack-studio \
  ; do
  echo "-- cargo check -p $pkg --"
  if ! cargo check -p "$pkg"; then
    fail "cargo check -p $pkg (default features) failed"
  fi
done

step "[5/6] decimal-bigdecimal reaches the whole graph cleanly through both server facades"
# The cratestack#495 acceptance bar in one command: `rust_decimal` must not
# be reachable anywhere in either facade's resolved dependency graph once
# `decimal-bigdecimal` is selected. `assert_no_rust_decimal` (called from
# `check_combo` above for every `*decimal-bigdecimal*` combo) already
# covers this per-package; these two are the exact commands the issue's
# acceptance bar names.
assert_no_rust_decimal cratestack-client decimal-bigdecimal
assert_no_rust_decimal cratestack-pg postgres,decimal-bigdecimal

step "[6/6] wasm32 targets: the wasm-only backend paths this feature graph flows through"
# `cratestack-rusqlite` swaps its FFI to `sqlite-wasm-rs` under
# target.'cfg(target_arch = "wasm32")', and `cratestack-cbor-wasm`'s
# wasm-bindgen glue (including the `cratestack-core` re-exports it uses)
# only compiles under `#[cfg(target_arch = "wasm32")]` in its own source —
# a native-only run of steps [3]/[4] never exercises either path, so the
# explicit `cratestack-core`/`cratestack-rusqlite` feature forwards have to
# be checked against the actual wasm32 target to mean anything for the
# crates that ship it. Both backends get the same wasm32 coverage.
if ! cargo check -p cratestack-sqlite --target wasm32-unknown-unknown --no-default-features --features decimal-rust-decimal; then
  fail "cargo check -p cratestack-sqlite --target wasm32-unknown-unknown --no-default-features --features decimal-rust-decimal failed"
fi
if ! cargo check -p cratestack-sqlite --target wasm32-unknown-unknown --no-default-features --features decimal-bigdecimal; then
  fail "cargo check -p cratestack-sqlite --target wasm32-unknown-unknown --no-default-features --features decimal-bigdecimal failed"
fi
if ! cargo check -p cratestack-cbor-wasm --target wasm32-unknown-unknown; then
  fail "cargo check -p cratestack-cbor-wasm --target wasm32-unknown-unknown failed"
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo
  echo "feature-matrix: FAILED — see ::error:: lines above" >&2
  exit 1
fi

echo
echo "feature-matrix: all checks passed"
