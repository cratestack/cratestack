#!/usr/bin/env bash
# Fixture tests for sibling_pins_check.py (cratestack#1390).
#
# Usage: sibling-pins-tests.sh
#
# Each case builds a tiny workspace in a temp dir (a root Cargo.toml plus
# optional member manifests) and runs the guard against it. Every shape of
# non-exact requirement must fail; workspace inheritance, unpublished members
# and exact pins must pass. Needs only bash and python3.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK="$HERE/sibling_pins_check.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0
n=0

# new_fixture <members-toml-array> -> echoes dir. Root has one exact entry so
# the guard's "found nothing" tripwire stays quiet unless a case wants it.
new_fixture() {
  n=$((n + 1))
  local d="$TMP/f$n"
  mkdir -p "$d"
  cat >"$d/Cargo.toml" <<TOML
[workspace]
members = $1
[workspace.package]
version = "0.20.1"
[workspace.dependencies]
cratestack-core = { path = "crates/core", version = "=0.20.1" }
TOML
  echo "$d"
}

member() { # member <dir> <name> <body...>
  local d="$1" name="$2"
  shift 2
  mkdir -p "$d/crates/$name"
  printf '[package]\nname = "%s"\nversion = "0.20.1"\n%s\n' "$name" "$*" >"$d/crates/$name/Cargo.toml"
}

root_deps() { # replace the workspace dependencies body
  local d="$1"
  shift
  printf '[workspace]\nmembers = ["crates/*"]\n[workspace.package]\nversion = "0.20.1"\n[workspace.dependencies]\n%s\n' "$*" >"$d/Cargo.toml"
}

expect() { # expect <fail|pass> <name> <dir> [grep-pattern-in-output]
  local want="$1" name="$2" d="$3" pat="${4:-}" out rc
  out="$(python3 "$CHECK" "$d" 2>&1)"
  rc=$?
  local ok=1
  if [ "$want" = fail ] && [ "$rc" -eq 0 ]; then ok=0; fi
  if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then ok=0; fi
  if [ -n "$pat" ] && ! grep -qF -- "$pat" <<<"$out"; then ok=0; fi
  if [ "$ok" -eq 1 ]; then
    pass=$((pass + 1))
    echo "ok   - $name"
  else
    fail=$((fail + 1))
    echo "FAIL - $name (wanted $want, exit $rc)" >&2
    echo "$out" | sed 's/^/       /' >&2
  fi
}

# --- non-exact shapes, in the workspace table, as string and as table -------
for req in '^0.20.1' '~0.20.1' '>=0.20.1' '0.20' '0.20.1'; do
  d=$(new_fixture '["crates/*"]')
  root_deps "$d" "cratestack-core = \"$req\""
  expect fail "workspace string \"$req\"" "$d"
  d=$(new_fixture '["crates/*"]')
  root_deps "$d" "cratestack-core = { path = \"crates/core\", version = \"$req\" }"
  expect fail "workspace table \"$req\"" "$d"
  d=$(new_fixture '["crates/*"]')
  member "$d" m
  printf '[dependencies]\ncratestack-core = "%s"\n' "$req" >>"$d/crates/m/Cargo.toml"
  expect fail "member string \"$req\"" "$d"
  d=$(new_fixture '["crates/*"]')
  member "$d" m
  printf '[dependencies]\ncratestack-core = { version = "%s" }\n' "$req" >>"$d/crates/m/Cargo.toml"
  expect fail "member table \"$req\"" "$d"
done

# --- other spellings ---------------------------------------------------------
d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies.cratestack-core]\nversion = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "dotted [dependencies.cratestack-core] table" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\nfacade = { package = "cratestack-api", version = "0.20.1" }\n' >>"$d/crates/m/Cargo.toml"
expect fail "renamed dependency in a member" "$d"

d=$(new_fixture '["crates/*"]')
root_deps "$d" 'facade = { package = "cratestack-api", path = "crates/api", version = "0.20.1" }
cratestack-core = { path = "crates/core", version = "=0.20.1" }'
expect fail "renamed dependency in the workspace table" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[target.%s.dependencies]\ncratestack-core = "0.20.1"\n' "'cfg(unix)'" >>"$d/crates/m/Cargo.toml"
expect fail "target-specific dependencies" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[target.%s.dev-dependencies]\ncratestack-core = "0.20.1"\n' "'cfg(unix)'" >>"$d/crates/m/Cargo.toml"
expect fail "target-specific dev-dependencies" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dev-dependencies]\ncratestack-core = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "[dev-dependencies]" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[build-dependencies]\ncratestack-core = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "[build-dependencies]" "$d"

# --- scope -------------------------------------------------------------------
d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\ncratestack = "0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "bare cratestack (vitrine) name" "$d"

d=$(new_fixture '["crates/*"]')
root_deps "$d" 'cratestack = { path = "crates/cratestack", version = "^0.20.1" }
cratestack-core = { path = "crates/core", version = "=0.20.1" }'
expect fail "bare cratestack in the workspace table" "$d"

d=$(new_fixture '["examples/*/app", "tools/one"]')
mkdir -p "$d/examples/x/app" "$d/tools/one"
printf '[package]\nname = "a"\nversion = "0.1.0"\n[dependencies]\ncratestack-core = "0.20.1"\n' >"$d/examples/x/app/Cargo.toml"
expect fail "member outside crates/* reached through a glob" "$d"

d=$(new_fixture '["tools/one"]')
mkdir -p "$d/tools/one"
printf '[package]\nname = "a"\nversion = "0.1.0"\n[dependencies]\ncratestack-core = "0.20.1"\n' >"$d/tools/one/Cargo.toml"
expect fail "member outside crates/* named literally" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\nserde = "1"\nother = { package = "cratestack-fake-not", version = "9" }\n' >>"$d/crates/m/Cargo.toml"
expect fail "any cratestack-* package name" "$d"

# --- fix hint ------------------------------------------------------------------
d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\ncratestack-core = ">=0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "fix hint prints the workspace version" "$d" 'use "=0.20.1"'
out="$(python3 "$CHECK" "$d" 2>&1)"
if grep -qF '=>=' <<<"$out"; then
  fail=$((fail + 1)); echo "FAIL - fix hint contains '=>='" >&2
else
  pass=$((pass + 1)); echo "ok   - fix hint never contains '=>='"
fi

# --- must pass -----------------------------------------------------------------
d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\ncratestack-core = { workspace = true }\n[dev-dependencies]\ncratestack-core.workspace = true\n' >>"$d/crates/m/Cargo.toml"
expect pass "workspace = true inherits the checked entry" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m 'publish = false'
printf '[dependencies]\ncratestack-core = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect pass "publish = false member is skipped" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m 'publish = []'
printf '[dependencies]\ncratestack-core = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect pass "publish = [] member is skipped" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m 'publish = ["my-registry"]'
printf '[dependencies]\ncratestack-core = "^0.20.1"\n' >>"$d/crates/m/Cargo.toml"
expect fail "publish = [registry] member is still checked" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
printf '[dependencies]\ncratestack-core = "=0.20.1"\ncratestack = { package = "cratestack-pg", version = "=0.20.1" }\ncratestack-x = { path = "../x" }\n' >>"$d/crates/m/Cargo.toml"
printf '[target.%s.dependencies]\ncratestack-core = "=0.20.1"\n' "'cfg(unix)'" >>"$d/crates/m/Cargo.toml"
expect pass "exact pins and a path-only sibling" "$d"

d=$(new_fixture '["crates/*"]'); member "$d" m
mkdir -p "$d/crates/skipped"
printf '[dependencies]\ncratestack-core = "0.20.1"\n' >"$d/crates/skipped/Cargo.toml"
sed -i 's|members = \["crates/\*"\]|members = ["crates/*"]\nexclude = ["crates/skipped"]|' "$d/Cargo.toml"
expect pass "excluded directory is not scanned" "$d"

d=$(new_fixture '["crates/*"]')
root_deps "$d" 'serde = "1"'
expect fail "no cratestack entries at all trips the tripwire" "$d"

echo
echo "sibling-pins tests: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
