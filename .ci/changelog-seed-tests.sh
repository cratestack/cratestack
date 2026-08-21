#!/usr/bin/env bash
# Tests for changelog-seed.sh and changelog-check.sh
#
# Usage: changelog-seed-tests.sh
#
# These tests verify that:
# 1. changelog-seed creates a new section with the correct format, in the
#    correct position, without disturbing any leading front matter
# 2. changelog-seed refuses to overwrite existing sections
# 3. changelog-check detects unedited seeds
# 4. changelog-check passes when seeds are edited
# 5. the range/commit the seed was computed from is persisted, not just echoed
# 6. the real, tracked CHANGELOG.md is never touched by any of the above —
#    every scenario runs against an isolated sandbox copy via the
#    CHANGELOG_FILE override both scripts respect
# 7. (cratestack#531) an existing "## Unreleased" section with real prose is
#    converted into the new dated release section — prose carried forward,
#    no seed, and no stale "## Unreleased" heading left behind
# 8. (cratestack#531) an existing but EMPTY "## Unreleased" section falls
#    back to the seed, consumed in place (no stale empty heading left behind)
# 9. (cratestack#531) no "## Unreleased" section at all still falls back to
#    the original "insert above the current newest entry" behavior
# 10-12. (cratestack#531) numbered variants of 7-9 above against different
#    fixtures — see each test's own header.
# 13-16. (cratestack#650) the changelog set is a declared LIST, not a single
#    hardcoded path: changelog-seed seeds every file in the set, refuses
#    atomically (no partial seed) if any file already has the version, and
#    changelog-check names exactly which file(s) in the set are unedited.
# 17. (cratestack#650 — the ticket's named risk) the pre-existing single-file
#    CHANGELOG_FILE override still works unchanged and takes precedence —
#    the multi-file mechanism must not change what that override means.
# 18. (cratestack#650) the production declared set in changelog-files.sh
#    actually lists both the root changelog and cratestack_cbor's.
#
# Tests 1, 3, and 8 (original numbering) deliberately do NOT run against a
# copy of the real, tracked CHANGELOG.md — its top section is "## Unreleased"
# by this repo's own PR convention (individual PRs add prose there as they
# land), which is exactly the state the cratestack#531 fix now special-cases.
# Running the "plain seed" assertions against that real, moving-target
# content would make those tests fail (correctly — the seed path doesn't run
# when there's real prose to carry forward — see the new Test 7 below) for
# the wrong reason: not a regression, but the fixture no longer matching
# what the test was written to exercise. They instead write a small,
# self-contained fixture with no "## Unreleased" heading, so they stay
# deterministic regardless of what the real CHANGELOG.md's Unreleased
# section currently contains.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REAL_CHANGELOG="$REPO_ROOT/CHANGELOG.md"
SEED_SCRIPT="$REPO_ROOT/.ci/changelog-seed.sh"
CHECK_SCRIPT="$REPO_ROOT/.ci/changelog-check.sh"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PASS_COUNT=0
FAIL_COUNT=0
TEST_DIR=""
SANDBOX_CHANGELOG=""
REPLY_OUT=""
REPLY_STATUS=0

test_pass() {
  echo -e "${GREEN}✓${NC} $1"
  PASS_COUNT=$((PASS_COUNT + 1))
}

test_fail() {
  echo -e "${RED}✗${NC} $1"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

test_header() {
  echo -e "\n${YELLOW}=== $1 ===${NC}"
}

# Runs $@ with CHANGELOG_FILE pointed at the sandbox copy, capturing combined
# stdout+stderr into REPLY_OUT and the exit code into REPLY_STATUS. Deliberately
# NOT `cmd | grep ...`: changelog-seed.sh/changelog-check.sh legitimately exit
# non-zero for several scenarios under test (refusing an overwrite, detecting
# an unedited seed), and under `set -o pipefail` a pipeline's status is the
# *rightmost non-zero* exit code — so `failing_cmd | grep -q match` reports
# pipeline failure even when grep DID match, silently breaking the assertion.
# Capturing via command substitution sidesteps that entirely.
run_capture() {
  REPLY_STATUS=0
  REPLY_OUT=$(CHANGELOG_FILE="$SANDBOX_CHANGELOG" "$@" 2>&1) || REPLY_STATUS=$?
}

# Same as run_capture, but exercises the multi-file path via
# CHANGELOG_FILES_OVERRIDE (a newline-separated list of sandbox paths)
# instead of the single-file CHANGELOG_FILE override — for the multi-file
# test cases below. $1 is the newline-separated list, the rest is the
# command to run.
run_capture_multi() {
  local override="$1"
  shift
  REPLY_STATUS=0
  REPLY_OUT=$(CHANGELOG_FILES_OVERRIDE="$override" "$@" 2>&1) || REPLY_STATUS=$?
}

setup_test() {
  TEST_DIR=$(mktemp -d)
  SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
  cp "$REAL_CHANGELOG" "$SANDBOX_CHANGELOG"
}

# Same as setup_test, but writes a small self-contained fixture with NO
# "## Unreleased" heading, instead of copying the real (moving-target)
# CHANGELOG.md — see the file header comment on why Tests 1/3/8 need this.
setup_test_no_unreleased() {
  TEST_DIR=$(mktemp -d)
  SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
  cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE
}

cleanup_test() {
  rm -rf "$TEST_DIR"
}
trap cleanup_test EXIT

# Test 1: Seed creates section with correct format, in the correct position
test_header "Test 1: changelog-seed creates a new section"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ] && echo "$REPLY_OUT" | grep -q "seeded with section for 0.9.9"; then
  if grep -q "^## 0.9.9 (" "$SANDBOX_CHANGELOG"; then
    test_pass "New section added with correct heading format"
  else
    test_fail "Section heading not found"
  fi

  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_pass "TODO marker present in new section"
  else
    test_fail "TODO marker not found"
  fi

  # The pre-existing "# Changelog" H1 title must stay at the top of the file —
  # not get buried mid-document by the new section landing above it.
  if head -1 "$SANDBOX_CHANGELOG" | grep -q "^# Changelog"; then
    test_pass "Leading '# Changelog' title is preserved at the top of the file"
  else
    test_fail "Leading title was displaced from the top of the file"
  fi

  # The new section must land above the previous newest entry (## 0.7.8).
  new_line=$(grep -n "^## 0.9.9 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  old_line=$(grep -n "^## 0.7.8 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  if [ -n "$new_line" ] && [ -n "$old_line" ] && [ "$new_line" -lt "$old_line" ]; then
    test_pass "New section positioned above the previous newest entry"
  else
    test_fail "New section not positioned above the previous newest entry"
  fi
else
  test_fail "changelog-seed script failed"
fi

cleanup_test

# Test 2: Seed refuses to overwrite existing section
test_header "Test 2: changelog-seed refuses to overwrite existing section"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -ne 0 ]; then
  test_fail "First seed of 0.9.9 unexpectedly failed: $REPLY_OUT"
fi

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "already contains a section"; then
  test_pass "Correctly refuses to overwrite existing section"
else
  test_fail "Should have refused to overwrite existing section"
fi

cleanup_test

# Test 3: changelog-check detects unedited seeds
test_header "Test 3: changelog-check detects unedited seeds"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9

run_capture "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "contain unedited seed"; then
  test_pass "changelog-check correctly detected unedited seed"
else
  test_fail "changelog-check should have detected unedited seed"
fi

cleanup_test

# Test 4: changelog-check passes when seed is edited
test_header "Test 4: changelog-check passes when seed is edited"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
sed -i '/<!-- TODO: edit this section from the seed below -->/d' "$SANDBOX_CHANGELOG"

run_capture "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -eq 0 ]; then
  test_pass "changelog-check passes after TODO marker removed"
else
  test_fail "changelog-check should pass after seed is edited: $REPLY_OUT"
fi

cleanup_test

# Test 5: Seed includes commits grouped by type
test_header "Test 5: changelog-seed groups commits by conventional-commit type"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  type_sections=$(grep -c "^#### " "$SANDBOX_CHANGELOG" || true)
  if [ "$type_sections" -gt 0 ]; then
    test_pass "Seed includes type groupings (found $type_sections sections)"
  else
    test_fail "No type groupings found in seed"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

cleanup_test

# Test 6: Seed formats date correctly (YYYY-MM-DD)
test_header "Test 6: changelog-seed formats date correctly"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -E "^## 0.9.9 \([0-9]{4}-[0-9]{2}-[0-9]{2}\)" "$SANDBOX_CHANGELOG" >/dev/null; then
    test_pass "Date formatted correctly (YYYY-MM-DD)"
  else
    test_fail "Date format is incorrect"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

cleanup_test

# Test 7: Seed refuses invalid version format
test_header "Test 7: changelog-seed refuses invalid version format"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" v0.9.9
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "VERSION must be X.Y.Z format"; then
  test_pass "Correctly refuses version with 'v' prefix"
else
  test_fail "Should refuse version with 'v' prefix"
fi

run_capture "$SEED_SCRIPT" 0.9
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "VERSION must be X.Y.Z format"; then
  test_pass "Correctly refuses incomplete version"
else
  test_fail "Should refuse incomplete version"
fi

cleanup_test

# Test 8: Seed persists the range/commit it was computed from, in the file
# itself — not just on stdout. Acceptance criterion: "the seed states the
# commit it was computed from, so an omission is detectable."
test_header "Test 8: changelog-seed embeds the computed range/commit in the file"
setup_test_no_unreleased

run_capture "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -qE '^<!-- seeded from .+ at [0-9a-f]{7,40} -->$' "$SANDBOX_CHANGELOG"; then
    test_pass "Range/commit marker persisted in CHANGELOG.md"
  else
    test_fail "Range/commit marker not found in CHANGELOG.md"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

cleanup_test

# Test 10 (cratestack#531 — THE DECISIVE TEST for defect 2): an existing
# "## Unreleased" section with real prose is converted into the new dated
# release section — the prose ends up under the new heading, verbatim, and
# no "## Unreleased" heading remains anywhere in the file. This is the exact
# scenario that shipped v0.7.12 with an unedited seed: prose written by PRs
# landed since the last release, stranded under a stale "## Unreleased"
# below the release section instead of folded into it.
test_header "Test 10 (cratestack#531): existing '## Unreleased' prose is carried forward"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## Unreleased

### Real narrative prose landed by PR #900

This is genuine, hand-written prose describing a real change, not a seed
placeholder. It spans multiple lines to look like actual changelog content.

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 1.3.0
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "^## 1.3.0 (" "$SANDBOX_CHANGELOG"; then
    test_pass "New dated heading created"
  else
    test_fail "New dated heading not found"
  fi

  if grep -q "^## Unreleased$" "$SANDBOX_CHANGELOG"; then
    test_fail "Stale '## Unreleased' heading still present — prose was NOT converted in place"
  else
    test_pass "No stale '## Unreleased' heading remains"
  fi

  if grep -q "Real narrative prose landed by PR #900" "$SANDBOX_CHANGELOG"; then
    test_pass "Carried-forward prose is present in the output"
  else
    test_fail "Carried-forward prose is missing from the output"
  fi

  # The prose must be UNDER the new heading, not left trailing below the
  # older release section (i.e. genuinely converted, not just left in place
  # with an unrelated new empty section inserted above it).
  new_heading_line=$(grep -n "^## 1.3.0 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  prose_line=$(grep -n "Real narrative prose landed by PR #900" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  old_heading_line=$(grep -n "^## 1.2.3 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  if [ -n "$new_heading_line" ] && [ -n "$prose_line" ] && [ -n "$old_heading_line" ] \
    && [ "$new_heading_line" -lt "$prose_line" ] && [ "$prose_line" -lt "$old_heading_line" ]; then
    test_pass "Carried-forward prose sits between the new heading and the previous release section"
  else
    test_fail "Carried-forward prose is not positioned correctly relative to the headings"
  fi

  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_fail "Seed marker present even though real prose existed to carry forward"
  else
    test_pass "No seed marker written — real prose made the seed unnecessary"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"

# Test 11 (cratestack#531): an existing but EMPTY "## Unreleased" section —
# the state right after a release, before any PR has landed — falls back to
# the seed, consumed in place. No stale empty "## Unreleased" heading should
# be left stranded below the release section either.
test_header "Test 11 (cratestack#531): empty '## Unreleased' section falls back to the seed"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## Unreleased

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 1.3.0
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_pass "Seed marker written — nothing to carry forward, so the seed was used"
  else
    test_fail "Seed marker missing even though '## Unreleased' was empty"
  fi

  if grep -q "^## Unreleased$" "$SANDBOX_CHANGELOG"; then
    test_fail "Stale empty '## Unreleased' heading still present"
  else
    test_pass "No stale '## Unreleased' heading remains"
  fi

  if grep -q "^## 1.3.0 (" "$SANDBOX_CHANGELOG"; then
    test_pass "New dated heading created"
  else
    test_fail "New dated heading not found"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"

# Test 12 (cratestack#531): no "## Unreleased" section at all — the original
# "insert above the current newest entry" behavior must still apply
# unchanged (this is the pre-#531 behavior every earlier test already
# exercises; asserted explicitly here too as a direct regression guard).
test_header "Test 12 (cratestack#531): absent '## Unreleased' section falls back to the original insert behavior"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 1.3.0
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_pass "Seed marker written — no '## Unreleased' section existed to carry forward"
  else
    test_fail "Seed marker missing even though no '## Unreleased' section existed"
  fi

  new_line=$(grep -n "^## 1.3.0 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  old_line=$(grep -n "^## 1.2.3 (" "$SANDBOX_CHANGELOG" | head -1 | cut -d: -f1)
  if [ -n "$new_line" ] && [ -n "$old_line" ] && [ "$new_line" -lt "$old_line" ]; then
    test_pass "New section positioned above the previous newest entry"
  else
    test_fail "New section not positioned above the previous newest entry"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""
SANDBOX_CHANGELOG=""

# Test 13 (cratestack#650): changelog-seed seeds EVERY file in a multi-file
# set, not just one. Two independent sandbox fixtures, neither touching the
# real, tracked changelogs, wired together via CHANGELOG_FILES_OVERRIDE.
test_header "Test 13 (cratestack#650): changelog-seed seeds every file in a multi-file set"
TEST_DIR=$(mktemp -d)
ROOT_FIXTURE="$TEST_DIR/root/CHANGELOG.md"
PKG_FIXTURE="$TEST_DIR/pkg/CHANGELOG.md"
mkdir -p "$TEST_DIR/root" "$TEST_DIR/pkg"
cat > "$ROOT_FIXTURE" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE
cat > "$PKG_FIXTURE" <<'FIXTURE'
## 0.8.3

Some previously released, already-edited prose.
FIXTURE

run_capture_multi "$ROOT_FIXTURE"$'\n'"$PKG_FIXTURE" "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "^## 0.9.9 (" "$ROOT_FIXTURE" && grep -q "TODO: edit this section from the seed below" "$ROOT_FIXTURE"; then
    test_pass "First declared file (root fixture) received a seeded section"
  else
    test_fail "First declared file (root fixture) did not receive a seeded section"
  fi

  if grep -q "^## 0.9.9 (" "$PKG_FIXTURE" && grep -q "TODO: edit this section from the seed below" "$PKG_FIXTURE"; then
    test_pass "Second declared file (package fixture) received a seeded section"
  else
    test_fail "Second declared file (package fixture) did not receive a seeded section"
  fi
else
  test_fail "changelog-seed failed on the multi-file set: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 14 (cratestack#650): multi-file seeding is atomic — if ANY declared
# file already has a section for the version, changelog-seed must refuse
# before writing to ANY of them, not seed the first file(s) and then fail
# partway through the set.
test_header "Test 14 (cratestack#650): changelog-seed refuses atomically across a multi-file set"
TEST_DIR=$(mktemp -d)
ROOT_FIXTURE="$TEST_DIR/root/CHANGELOG.md"
PKG_FIXTURE="$TEST_DIR/pkg/CHANGELOG.md"
mkdir -p "$TEST_DIR/root" "$TEST_DIR/pkg"
cat > "$ROOT_FIXTURE" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE
# The second file already has a section for the version being seeded
# (heading format mirrors the real seeded/edited style — a trailing space
# and date parenthetical after the version, matching the `^## $VERSION `
# pattern both scripts grep for).
cat > "$PKG_FIXTURE" <<'FIXTURE'
## 0.9.9 (2020-01-01)

Already released by hand.
FIXTURE

run_capture_multi "$ROOT_FIXTURE"$'\n'"$PKG_FIXTURE" "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "already contains a section"; then
  test_pass "changelog-seed correctly refused when the second file already had the section"
else
  test_fail "changelog-seed should have refused: status=$REPLY_STATUS out=$REPLY_OUT"
fi

if grep -q "^## 0.9.9 (" "$ROOT_FIXTURE"; then
  test_fail "First file was seeded even though the set was refused — not atomic"
else
  test_pass "First file was left untouched — refusal is atomic across the set"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 15 (cratestack#650): changelog-check, given a multi-file set, exits
# non-zero and names the SPECIFIC file(s) still holding the unedited seed
# marker — not just "something is wrong", and not the edited file too.
test_header "Test 15 (cratestack#650): changelog-check names which file is unedited"
TEST_DIR=$(mktemp -d)
ROOT_FIXTURE="$TEST_DIR/root/CHANGELOG.md"
PKG_FIXTURE="$TEST_DIR/pkg/CHANGELOG.md"
mkdir -p "$TEST_DIR/root" "$TEST_DIR/pkg"
cat > "$ROOT_FIXTURE" <<'FIXTURE'
# Changelog

## 0.9.9 (2026-08-21)

Already edited, narrative prose. No seed marker here.
FIXTURE
cat > "$PKG_FIXTURE" <<'FIXTURE'
## 0.9.9 (2026-08-21)

<!-- TODO: edit this section from the seed below -->
<!-- seeded from HEAD..HEAD at 0000000 -->

This is an auto-generated seed. Please rewrite into narrative prose.
FIXTURE

run_capture_multi "$ROOT_FIXTURE"$'\n'"$PKG_FIXTURE" "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "contain unedited seed"; then
  test_pass "changelog-check correctly failed with an unedited seed in the set"
else
  test_fail "changelog-check should have failed: status=$REPLY_STATUS out=$REPLY_OUT"
fi

if echo "$REPLY_OUT" | grep -qF "$PKG_FIXTURE"; then
  test_pass "changelog-check named the specific unedited file (package fixture)"
else
  test_fail "changelog-check did not name the unedited file in its output"
fi

if echo "$REPLY_OUT" | grep -qF "$ROOT_FIXTURE"; then
  test_fail "changelog-check incorrectly named the already-edited file too"
else
  test_pass "changelog-check did not name the already-edited file"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 16 (cratestack#650): changelog-check passes when every file in a
# multi-file set is edited (no unedited seed markers anywhere in the set).
test_header "Test 16 (cratestack#650): changelog-check passes when the whole multi-file set is edited"
TEST_DIR=$(mktemp -d)
ROOT_FIXTURE="$TEST_DIR/root/CHANGELOG.md"
PKG_FIXTURE="$TEST_DIR/pkg/CHANGELOG.md"
mkdir -p "$TEST_DIR/root" "$TEST_DIR/pkg"
cat > "$ROOT_FIXTURE" <<'FIXTURE'
# Changelog

## 0.9.9 (2026-08-21)

Already edited, narrative prose.
FIXTURE
cat > "$PKG_FIXTURE" <<'FIXTURE'
## 0.9.9 (2026-08-21)

Already edited, narrative prose too.
FIXTURE

run_capture_multi "$ROOT_FIXTURE"$'\n'"$PKG_FIXTURE" "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -eq 0 ]; then
  test_pass "changelog-check passes when every file in the set is edited"
else
  test_fail "changelog-check should pass when the whole set is edited: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 17 (cratestack#650 — THE degenerate-case guard from the ticket's named
# risk): the pre-existing single-file CHANGELOG_FILE override must keep
# working exactly as before, even now that a multi-file mechanism exists —
# it must win over CHANGELOG_FILES_OVERRIDE if both are somehow set, and
# operate on exactly one file. This is what stops "convert to a list" from
# silently changing what changelog-seed-test itself was exercising all
# along (Tests 1-12 above all rely on this single-file override).
test_header "Test 17 (cratestack#650): single-file CHANGELOG_FILE override remains the degenerate case"
setup_test_no_unreleased

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILE="$SANDBOX_CHANGELOG" CHANGELOG_FILES_OVERRIDE="/nonexistent/a.md"$'\n'"/nonexistent/b.md" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -eq 0 ] && grep -q "^## 0.9.9 (" "$SANDBOX_CHANGELOG"; then
  test_pass "CHANGELOG_FILE wins over CHANGELOG_FILES_OVERRIDE and seeds exactly the one sandbox file"
else
  test_fail "Single-file CHANGELOG_FILE override did not take precedence: status=$REPLY_STATUS out=$REPLY_OUT"
fi

cleanup_test

# Test 18 (cratestack#650): the declared set in changelog-files.sh is the
# single source of truth, and it must actually list both the root changelog
# and dart-packages/cratestack_cbor's — a static assertion (no mutation of
# any tracked file) that the production wiring matches the acceptance
# criterion, independent of the sandbox-only tests above.
test_header "Test 18 (cratestack#650): the declared set includes the root and cratestack_cbor changelogs"
CHANGELOG_FILES_DEFAULT=()
# shellcheck source=/dev/null
source "$REPO_ROOT/.ci/changelog-files.sh"

has_root=false
has_cbor=false
for f in "${CHANGELOG_FILES_DEFAULT[@]}"; do
  [ "$f" = "CHANGELOG.md" ] && has_root=true
  [ "$f" = "dart-packages/cratestack_cbor/CHANGELOG.md" ] && has_cbor=true
done

if [ "$has_root" = "true" ]; then
  test_pass "Declared set includes the root CHANGELOG.md"
else
  test_fail "Declared set is missing the root CHANGELOG.md"
fi

if [ "$has_cbor" = "true" ]; then
  test_pass "Declared set includes dart-packages/cratestack_cbor/CHANGELOG.md"
else
  test_fail "Declared set is missing dart-packages/cratestack_cbor/CHANGELOG.md"
fi

# Test 9: none of the above ever touches the real, tracked changelogs.
# This guards the sandbox-escape regression directly: every test above must
# operate purely on sandbox copies via the CHANGELOG_FILE /
# CHANGELOG_FILES_OVERRIDE overrides — never the real, tracked files.
test_header "Test 9: no real, tracked changelog is ever modified"

if git -C "$REPO_ROOT" diff --quiet -- CHANGELOG.md 2>/dev/null; then
  test_pass "Real CHANGELOG.md has no working-tree changes after running the suite"
else
  test_fail "Real CHANGELOG.md was modified by the test suite — sandbox isolation broken"
fi

if git -C "$REPO_ROOT" diff --quiet -- dart-packages/cratestack_cbor/CHANGELOG.md 2>/dev/null; then
  test_pass "Real dart-packages/cratestack_cbor/CHANGELOG.md has no working-tree changes after running the suite"
else
  test_fail "Real dart-packages/cratestack_cbor/CHANGELOG.md was modified by the test suite — sandbox isolation broken"
fi

# Summary
echo -e "\n${YELLOW}=== Test Summary ===${NC}"
TOTAL=$((PASS_COUNT + FAIL_COUNT))
echo "Passed: $PASS_COUNT/$TOTAL"
echo "Failed: $FAIL_COUNT/$TOTAL"

if [ "$FAIL_COUNT" -eq 0 ]; then
  echo -e "\n${GREEN}All tests passed!${NC}"
  exit 0
else
  echo -e "\n${RED}Some tests failed.${NC}"
  exit 1
fi
