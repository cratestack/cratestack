#!/usr/bin/env bash
# Multi-file schema rebuild proof (cratestack#919, design §10 and §15).
#
# WHAT THIS PROVES, with real `cargo` builds and cargo's own freshness
# verdict (the `fresh` field of the `compiler-artifact` message), never by
# inspecting the macro output:
#
#   1. a no-op rebuild is fresh (so the detector can say "no");
#   2. editing ANY file of a multi-file schema (the root, a part, an import,
#      and a part of an import) makes the consuming crate rebuild, because
#      `include_schema!` emits one `include_str!` per file in `SCHEMA_FILES`;
#   3. adding a new part and listing it in the root rebuilds, the new file
#      lands in the test binary's dep-info (`.d`), and the new type is
#      visible to the compiled code;
#   4. THE DESIGNED GAP: a `part of` file that no root lists is NOT read by
#      the build, so adding one does not rebuild (design §10: a build cannot
#      report it without globbing, which the epic rejected).
#
# WHAT IT CANNOT PROVE: that every possible filesystem/mtime race is
# handled (it relies on cargo's mtime-based fingerprints, like every
# crate), nor that `include_str!` tracking works on a toolchain older than
# the stable one CI uses.
#
# It edits the committed fixture in place and restores it on exit, so it
# refuses to start over local edits to that directory.

set -euo pipefail

cd "$(dirname "$0")/.."

FIXTURE=crates/cratestack-sqlite/tests/fixtures/multi_file
TEST=multi_file_schema
LOG="$(mktemp)"
STEPS=()
FAILED=0

if ! git diff --quiet -- "$FIXTURE"; then
  echo "error: uncommitted changes under $FIXTURE; commit or stash them first" >&2
  exit 2
fi

cleanup() {
  git checkout -- "$FIXTURE" >/dev/null 2>&1 || true
  rm -f "$FIXTURE/parts/probe.cstack" "$FIXTURE/parts/orphan.cstack" "$LOG"
}
trap cleanup EXIT

# build: prints "<fresh> <executable>" for the $TEST test target.
build() {
  cargo test -p cratestack-sqlite --test "$TEST" --no-run \
    --message-format=json 2>"$LOG.err" | python3 -c '
import json, sys
# Read ALL of stdin before exiting: returning on the first match closes the
# pipe while cargo still has messages to write (e.g. build-finished), so
# cargo hits EPIPE ("Broken pipe"), exits non-zero, and pipefail fails us.
found = None
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    target = msg.get("target", {})
    if (found is None
            and msg.get("reason") == "compiler-artifact"
            and target.get("name") == "'"$TEST"'"
            and "test" in target.get("kind", [])):
        found = (str(msg["fresh"]).lower(), msg["executable"])
if found is None:
    sys.exit("no compiler-artifact for the test target")
print(*found)
' || { cat "$LOG.err" >&2; echo "error: build failed" >&2; exit 1; }
}

# expect LABEL WANT_FRESH GOT_FRESH
expect() {
  local label="$1" want="$2" got="$3" verdict=ok
  if [ "$want" != "$got" ]; then verdict=FAIL; FAILED=1; fi
  STEPS+=("$(printf '%-52s want fresh=%-5s got fresh=%-5s %s' "$label" "$want" "$got" "$verdict")")
}

fresh_of() { build | cut -d' ' -f1; }

touch_later() { sleep 1; }

echo ">> warm build"
build >/dev/null
expect "no-op rebuild" true "$(fresh_of)"

for file in schema.cstack parts/notes.cstack shared/common.cstack shared/common/status.cstack; do
  echo ">> edit $file"
  touch_later
  printf '\n// rebuild probe\n' >>"$FIXTURE/$file"
  expect "edit $file" false "$(fresh_of)"
  git checkout -- "$FIXTURE/$file"
  build >/dev/null
done

echo ">> add and list a new part"
touch_later
printf 'part of "../schema.cstack"\n\ntype RebuildProbe {\n  x Int\n}\n' >"$FIXTURE/parts/probe.cstack"
printf '\npart "parts/probe.cstack"\n' >>"$FIXTURE/schema.cstack"
read -r fresh exe < <(build)
expect "add + list parts/probe.cstack" false "$fresh"
if grep -q 'parts/probe.cstack' "$exe.d"; then
  expect "new part is in the dep-info (.d)" true true
else
  expect "new part is in the dep-info (.d)" true false
fi
if CRATESTACK_REBUILD_PROBE=1 cargo test -p cratestack-sqlite --test "$TEST" \
    rebuild_probe >"$LOG.err" 2>&1; then
  expect "new type RebuildProbe visible to compiled code" true true
else
  cat "$LOG.err" >&2
  expect "new type RebuildProbe visible to compiled code" true false
fi
git checkout -- "$FIXTURE/schema.cstack"
rm -f "$FIXTURE/parts/probe.cstack"
build >/dev/null

echo ">> add an unlisted part-of file (designed gap)"
touch_later
printf 'part of "../schema.cstack"\n\ntype Orphan {\n  x Int\n}\n' >"$FIXTURE/parts/orphan.cstack"
expect "unlisted parts/orphan.cstack does NOT rebuild" true "$(fresh_of)"

rm -f "$LOG.err"
echo
echo "multi-file rebuild check"
printf '  %s\n' "${STEPS[@]}"
if [ "$FAILED" -ne 0 ]; then
  echo "FAILED" >&2
  exit 1
fi
echo "OK"
