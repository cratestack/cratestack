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
#      `cratestack-parser`, `cratestack-macros`,
#      `cratestack-axum`, `cratestack-codec-cbor`, `cratestack-codec-json`
#      all gained the same `default-features = false` + explicit-forward
#      treatment `cratestack-core`/`cratestack-sqlx`/`cratestack-rusqlite`/
#      `cratestack-client-rust` already had — a single crate left pinning
#      `decimal-rust-decimal` anywhere in that closure re-forces it for the
#      whole graph, since Cargo features are additive and unify globally.
#   4. #505 Direction 2 (associated-type/marker shape — see
#      `docs/design/decimal-backend-additivity.md`) removes the "both
#      selected is a hard error" half of step 3's invariant: `RustDecimal`/
#      `BigDecimal` are now independently-gated names (never one shared
#      `Decimal` alias resolving ambiguously), `SqlValue::Decimal` holds a
#      `Box<dyn DecimalLike>` instead of a fixed concrete type, and
#      `cratestack-macros` picks which concrete type a given schema uses via
#      a `decimal = RustDecimal | BigDecimal` macro argument — a
#      schema-authored choice, not a Cargo feature — so two independent
#      dependents that each select a different backend feature (this
#      script's own `[2/7]` step, updated below) now compile together in
#      one graph instead of hitting the old `compile_error!`. The "neither
#      selected, and nothing needs one" and "no `rust_decimal` reachable
#      under `decimal-bigdecimal` alone" guarantees steps 2/3 established
#      are both still asserted below, unchanged.
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

step "[1/7] cratestack#505's decisive acceptance bar: the reporter's exact crate shape, and every deployable facade, compile with no decimal backend selected at all"
# `cratestack-api` is the reporter's own repro (cratestack#505,
# ADORSYS-GIS/webank-services#279): `default-features = false`,
# `provider = "none"`, no `Decimal` field anywhere. An earlier version of
# this fix only gated `cratestack-core`'s own `Decimal` — `cratestack-sql`'s
# `SqlValue::Decimal(cratestack_core::Decimal)` variant (and the matching
# arms it rippled into across `cratestack-rusqlite`/`cratestack-sqlx`) was
# still unconditional, so this exact command still failed, just with a
# worse error two crates away. All four facades that expose a decimal
# toggle, plus the two non-facade crates every one of them pulls in
# (`cratestack-axum`, `cratestack-studio`), are checked here with no
# decimal feature selected at all.
for check in \
  "cratestack-api|--no-default-features" \
  "cratestack-pg|--no-default-features --features postgres" \
  "cratestack-sqlite|--no-default-features" \
  "cratestack-client|--no-default-features" \
  "cratestack-axum|--no-default-features" \
  "cratestack-studio|--no-default-features" \
  ; do
  pkg="${check%%|*}"
  args="${check#*|}"
  echo "-- cargo check -p $pkg $args --"
  # shellcheck disable=SC2086 # $args is a deliberate word-split flag list
  if ! cargo check -p "$pkg" $args; then
    fail "cargo check -p $pkg $args failed — cratestack#505's SqlValue::Decimal gating may have regressed, or a new unconditional Decimal reference was added somewhere in $pkg's dependency closure"
  else
    echo "ok: $pkg compiles with no decimal backend selected"
  fi
done

step "[2/7] decimal-rust-decimal and decimal-bigdecimal are BOTH selectable at once as of cratestack#505 Direction 2; neither is NOT an error either (cratestack#505's earlier, milder half)"
# Until cratestack#505 Direction 2 landed, this asserted the OPPOSITE: that
# `cratestack-core --features decimal-rust-decimal,decimal-bigdecimal`
# together was a hard `compile_error!` (cratestack#495's original mutual-
# exclusion invariant). That invariant is exactly what cratestack#505
# reports as the defect — two independent dependents, each individually
# well-formed, each choosing a different backend, could not appear
# together in one build. Direction 2 (see `docs/design/decimal-backend-
# additivity.md` §7(b)/§10) fixes it by never asking `cratestack-core` to
# pick one: `RustDecimal`/`BigDecimal` are independently-gated names, and
# `SqlValue::Decimal` no longer pins a concrete type. This step now
# asserts the FIXED behavior — both together compile cleanly — deliberately
# inverted from its pre-#505 form; see this script's own step [1/7]-through-
# [2/7] preamble ("History" item 4) for the reasoning.
if OUTPUT=$(cargo check -p cratestack-core --no-default-features --features decimal-rust-decimal,decimal-bigdecimal 2>&1); then
  echo "ok: selecting both decimal backends at once compiles cleanly (cratestack#505 Direction 2)"
else
  fail "expected 'cargo check -p cratestack-core --features decimal-rust-decimal,decimal-bigdecimal' (both backends at once) to succeed as of cratestack#505 Direction 2, but it failed"
  echo "$OUTPUT"
fi
# `cargo test`, not just `cargo check`: proves both `RustDecimal` and
# `BigDecimal` are actually usable (not just present) together — see
# `both_decimal_backends_tests` in `src/decimal.rs`.
if OUTPUT=$(cargo test -p cratestack-core --no-default-features --features decimal-rust-decimal,decimal-bigdecimal 2>&1); then
  if ! grep -q "both_decimal_backends_tests::both_backends_selected_at_once_compiles_and_runs ... ok" <<<"$OUTPUT"; then
    fail "cargo test -p cratestack-core --features decimal-rust-decimal,decimal-bigdecimal succeeded, but the cratestack#505 Direction 2 regression test didn't run — check src/decimal.rs's cfg gating"
    echo "$OUTPUT"
  else
    echo "ok: cratestack-core's test suite (including the cratestack#505 Direction 2 regression test) runs with both decimal backends selected"
  fi
else
  fail "cargo test -p cratestack-core --features decimal-rust-decimal,decimal-bigdecimal failed"
  echo "$OUTPUT"
fi
# cratestack#505: this used to be a hard `compile_error!` too (the "neither"
# arm) — the exact break a consumer legitimately narrowing its graph via
# `default-features = false`, and never touching `Decimal`, hit in the
# wild. `cratestack-core` now compiles cleanly with `Decimal` (and anything
# that references it, e.g. `validate_range_decimal`) simply absent from its
# public surface in this configuration — see `src/decimal.rs`'s module doc
# for why a real `rust_decimal`-backed fallback isn't reachable here (it
# would require `rust_decimal` to stop being a Cargo-optional dependency,
# which breaks the [6/7] no-leak invariant below).
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

step "[3/7] cratestack-core compiles clean on each backend individually"
if ! cargo check -p cratestack-core; then
  fail "cargo check -p cratestack-core (default features) failed"
fi
if ! cargo check -p cratestack-core --no-default-features --features decimal-bigdecimal; then
  fail "cargo check -p cratestack-core --no-default-features --features decimal-bigdecimal failed"
fi

step "[4/7] facade crates: default features AND both narrowed backend selections (AC2/AC4 matrix)"
# (package, extra args...) — every facade that exposes its own decimal
# toggle, checked at its default feature set and at both backends
# individually, so leaks can't hide behind whichever feature set happens to
# be the default, and `decimal-bigdecimal` gets exactly the same coverage
# `decimal-rust-decimal` always has.
check_combo cratestack-pg
# `--features postgres` alone (no explicit decimal choice) used to be a
# DELIBERATE compile failure as of cratestack#495 — `cratestack-sqlx`'s
# query-builder support code bound `cratestack_core::Decimal` values
# unconditionally, so `postgres` structurally required *some* decimal
# backend. cratestack#505's follow-up (the reporter's own crate shape,
# `cratestack-api`/`provider = "none"` with `default-features = false`
# and no `Decimal` field anywhere, still failed even after this PR's
# first pass — the failure had just moved from `cratestack-core`'s own
# `compile_error!` into `cratestack-sql`'s unconditional
# `SqlValue::Decimal(cratestack_core::Decimal)` variant) closed that gap
# too: `cratestack-sql`'s `SqlValue::Decimal`/`IntoSqlValue for Decimal`,
# and the matching arms in `cratestack-rusqlite`/`cratestack-sqlx`, are
# now `#[cfg]`-gated the same way `cratestack-core`'s own `Decimal` is
# (see `cratestack-core/src/decimal.rs`'s module doc) — so a facade with
# no `Decimal` field anywhere in its schema, `postgres` included, no
# longer needs a decimal backend at all. This is the positive
# counterpart to [2/7]'s cratestack-core-level check: the same "neither
# is not an error" guarantee now holds all the way through the facade a
# real consumer actually depends on, not just the innermost crate.
echo "-- cargo check -p cratestack-pg --no-default-features --features postgres (expected to succeed, cratestack#505) --"
if ! cargo check -p cratestack-pg --no-default-features --features postgres; then
  fail "cargo check -p cratestack-pg --no-default-features --features postgres failed — cratestack#505's SqlValue::Decimal gating in cratestack-sql/-rusqlite/-sqlx may have regressed"
else
  echo "ok: postgres alone (no decimal choice) compiles cleanly (cratestack#505)"
  assert_no_default_leak cratestack-pg postgres
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

step "[5/7] plain crates: every cratestack-core edge this PR made explicit actually compiles"
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
  cratestack-lsp \
  cratestack-macros \
  cratestack-migrate \
  cratestack-mock-wiremock \
  cratestack-parser \
  cratestack-policy \
  cratestack-redis \
  cratestack-studio \
  ; do
  echo "-- cargo check -p $pkg --"
  if ! cargo check -p "$pkg"; then
    fail "cargo check -p $pkg (default features) failed"
  fi
done

step "[6/7] decimal-bigdecimal reaches the whole graph cleanly through both server facades"
# The cratestack#495 acceptance bar in one command: `rust_decimal` must not
# be reachable anywhere in either facade's resolved dependency graph once
# `decimal-bigdecimal` is selected. `assert_no_rust_decimal` (called from
# `check_combo` above for every `*decimal-bigdecimal*` combo) already
# covers this per-package; these two are the exact commands the issue's
# acceptance bar names.
assert_no_rust_decimal cratestack-client decimal-bigdecimal
assert_no_rust_decimal cratestack-pg postgres,decimal-bigdecimal

step "[7/7] wasm32 targets: the wasm-only backend paths this feature graph flows through"
# `cratestack-rusqlite` swaps its FFI to `sqlite-wasm-rs` under
# target.'cfg(target_arch = "wasm32")', and `cratestack-cbor-wasm`'s
# wasm-bindgen glue (including the `cratestack-core` re-exports it uses)
# only compiles under `#[cfg(target_arch = "wasm32")]` in its own source —
# a native-only run of steps [4/7]/[5/7] never exercises either path, so the
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
