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
if [ "$REPLY_STATUS" -eq 0 ] && echo "$REPLY_OUT" | grep -q "seeded CHANGELOG.md"; then
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
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "contains unedited seed"; then
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

# Test 9: none of the above ever touches the real, tracked CHANGELOG.md.
# This guards the sandbox-escape regression directly: every test above must
# operate purely on $SANDBOX_CHANGELOG via the CHANGELOG_FILE override.
test_header "Test 9: the real CHANGELOG.md is never modified"

if git -C "$REPO_ROOT" diff --quiet -- CHANGELOG.md 2>/dev/null; then
  test_pass "Real CHANGELOG.md has no working-tree changes after running the suite"
else
  test_fail "Real CHANGELOG.md was modified by the test suite — sandbox isolation broken"
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
