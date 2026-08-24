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
# 5. commits are grouped by conventional-commit type, correctly scoped to the
#    last-release-tag range — exercised against a disposable git fixture this
#    test builds itself (cratestack#670), not the ambient repo's
#    tags/history/HEAD position — and the range/commit the seed was computed
#    from is persisted in the file, not just echoed
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
# 19-26. (cratestack#688) after the seed script runs, every declared
#    changelog file ends with exactly one fresh, empty '## Unreleased'
#    heading immediately above the newest dated section, on all three
#    per-file branches and in the multi-file case — asserted by exact
#    whole-file diff, not substring grep, where the result is deterministic.
# 27-31. (cratestack#713) a declared package whose no-op scope (its own
#    directory PLUS the extra directories that can change what it ships
#    without touching that directory — see changelog-files.sh's
#    CHANGELOG_NOOP_SCOPES comment for why cratestack_cbor's scope reaches
#    into specific Rust crate directories) has zero non-bump commits in
#    range gets the standard "No functional changes" wording instead of the
#    placeholder — no manual edit, gate passes (27). The decisive
#    counterpart (28): a real change reaching the scope ONLY through one of
#    those extra directories, never the package's own, still writes the
#    placeholder and still fails the gate — proving the widened scope, not
#    just the package directory, is what's actually checked. A file
#    explicitly named in CHANGELOG_NOOP_EXEMPT (the root CHANGELOG.md, in
#    production) never takes this fallback (29). The production
#    CHANGELOG_NOOP_SCOPES/CHANGELOG_NOOP_EXEMPT are asserted directly (30),
#    mirroring Test 18 — now covering all three Dart packages, not just
#    cratestack_cbor. Test 31 is the coverage guard's own decisive test: a
#    declared changelog with neither a scope nor an exemption must fail
#    loudly (the omission that silently affected cratestack_annotations and
#    cratestack_builder after #714, until this PR's follow-up), and the
#    same declaration with the gap closed must not.
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

# Runs $@ with CHANGELOG_FILES_SOURCE pointed at an alternate
# changelog-files.sh declaration file — used to exercise the
# empty-declared-set guard against a fixture (e.g. a renamed/emptied
# CHANGELOG_FILES_DEFAULT) without touching the real, tracked
# .ci/changelog-files.sh. $1 is the path to the fixture file, the rest is
# the command to run.
run_capture_with_source() {
  local source_file="$1"
  shift
  REPLY_STATUS=0
  REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$source_file" "$@" 2>&1) || REPLY_STATUS=$?
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

# Test 5 (cratestack#670): Seed includes commits grouped by type, exercised
# against a self-contained, throwaway git fixture's own last-tag range — not
# the ambient repo's tags/history/HEAD position.
#
# The old version of this test asserted "at least one #### grouping exists"
# against a sandboxed CHANGELOG.md but the REAL, ambient git history (via
# changelog-seed.sh's own `git log "${last_tag}..HEAD"`, computed against
# whatever repo the test happened to run in). What that measured depended
# entirely on the checkout:
#   - Locally, on a full clone with HEAD exactly at the newest release tag
#     (right after a release-bump merge), the range is empty, so no
#     groupings are emitted and the assertion goes red.
#   - In CI, a shallow/tagless `actions/checkout` finds no tags, so
#     `last_tag` is empty and `range` silently degrades to plain `HEAD` —
#     the assertion passes against a one-commit log for a reason that has
#     nothing to do with the range logic being correct. It would keep
#     passing even if that logic were completely broken.
#
# Fixed by building a disposable git repository right here — a known commit
# BEFORE a known tag, and known commits AFTER it — and pointing
# changelog-seed.sh's git calls at that fixture via GIT_DIR/GIT_WORK_TREE.
# changelog-seed.sh always `cd`s to the real PROJECT_ROOT itself (see its
# own PROJECT_ROOT computation), so the CHANGELOG_FILE seam alone cannot
# relocate which repository `git log`/`git tag`/`git rev-parse` read from —
# GIT_DIR/GIT_WORK_TREE is the seam that can, and git respects it
# regardless of cwd. This makes the test deterministic on a full clone, a
# shallow/tagless clone, and immediately after a release bump alike, since
# none of those ambient properties are consulted at all.
#
# The decisive assertion is the negative one below: the pre-tag commit must
# NOT appear in the seed. That is what actually exercises the
# `last_tag`/range computation — a regression that computed the range from
# the repo root instead of from the last tag (or pointed at the wrong ref)
# would leak the pre-tag commit into the seed and this assertion would catch
# it; "some grouping appeared" alone would not.
test_header "Test 5 (cratestack#670): changelog-seed groups commits by conventional-commit type, from a self-contained git fixture's last-tag range"
setup_test_no_unreleased

GIT_FIXTURE_DIR=$(mktemp -d)
git init -q -b main "$GIT_FIXTURE_DIR"
git -C "$GIT_FIXTURE_DIR" config user.email "changelog-seed-tests@example.invalid"
git -C "$GIT_FIXTURE_DIR" config user.name "changelog-seed-tests"
git -C "$GIT_FIXTURE_DIR" config commit.gpgsign false
git -C "$GIT_FIXTURE_DIR" commit -q --allow-empty -m "feat: pre-tag commit that must not appear in the seed"
git -C "$GIT_FIXTURE_DIR" tag v1.0.0
git -C "$GIT_FIXTURE_DIR" commit -q --allow-empty -m "feat: post-tag feature commit"
git -C "$GIT_FIXTURE_DIR" commit -q --allow-empty -m "fix: post-tag fix commit"

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILE="$SANDBOX_CHANGELOG" GIT_DIR="$GIT_FIXTURE_DIR/.git" GIT_WORK_TREE="$GIT_FIXTURE_DIR" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -eq 0 ]; then
  type_sections=$(grep -c "^#### " "$SANDBOX_CHANGELOG" || true)
  if [ "$type_sections" -gt 0 ]; then
    test_pass "Seed includes type groupings (found $type_sections sections)"
  else
    test_fail "No type groupings found in seed"
  fi

  if grep -q "^#### Features$" "$SANDBOX_CHANGELOG" && grep -q "^#### Fixes$" "$SANDBOX_CHANGELOG"; then
    test_pass "Both known post-tag commit types (feat, fix) are grouped"
  else
    test_fail "Expected '#### Features' and '#### Fixes' groupings from the fixture's post-tag commits"
  fi

  if grep -q "post-tag feature commit" "$SANDBOX_CHANGELOG" && grep -q "post-tag fix commit" "$SANDBOX_CHANGELOG"; then
    test_pass "Both known post-tag commits are present in the seed"
  else
    test_fail "Expected post-tag commits missing from the seed"
  fi

  if grep -q "pre-tag commit that must not appear" "$SANDBOX_CHANGELOG"; then
    test_fail "Pre-tag commit leaked into the seed — last_tag/range computation is not scoping to commits since the last release tag"
  else
    test_pass "Pre-tag commit correctly excluded — range computation is scoped to commits since the last release tag"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

rm -rf "$GIT_FIXTURE_DIR"
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

  # cratestack#688: the OLD assertion here was that no "## Unreleased"
  # heading remained at all. That is no longer correct — a FRESH, empty one
  # is now expected immediately above the new dated heading (see Test 23
  # below for the dedicated, exact-content assertion of that invariant).
  # What must still hold from the original #531 behavior is that there is
  # exactly ONE "## Unreleased" heading (not a stale second one left behind
  # from the original section), and it no longer has the carried-forward
  # prose under it — the prose moved to the dated section, not duplicated.
  unreleased_count=$(grep -c "^## Unreleased$" "$SANDBOX_CHANGELOG" || true)
  if [ "$unreleased_count" -eq 1 ]; then
    test_pass "Exactly one '## Unreleased' heading present (the freshly re-seeded one, not a stale leftover)"
  else
    test_fail "Expected exactly one '## Unreleased' heading, found $unreleased_count"
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

  # cratestack#688: the OLD assertion here was that no "## Unreleased"
  # heading remained at all — that was the bug this issue fixes. A fresh,
  # empty one is now expected above the seeded section (Test 24 below
  # asserts the exact content). What must still hold is that the ORIGINAL
  # empty heading was consumed in place, not left stranded as a SECOND
  # heading alongside the freshly re-seeded one.
  unreleased_count=$(grep -c "^## Unreleased$" "$SANDBOX_CHANGELOG" || true)
  if [ "$unreleased_count" -eq 1 ]; then
    test_pass "Exactly one '## Unreleased' heading present (re-seeded, not a stale leftover of the consumed one)"
  else
    test_fail "Expected exactly one '## Unreleased' heading, found $unreleased_count"
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

# Test 19 (cratestack#650 — reviewer-caught footgun): CHANGELOG_FILES_OVERRIDE
# strings with a trailing newline or an embedded blank line must not produce
# a bogus empty-path array element. Before the fix this surfaced as a bare
# "error:  not found" with no filename; both scripts now skip blank lines
# when splitting the override instead of using `mapfile ... <<<` directly
# (a here-string always appends its own trailing newline).
test_header "Test 19 (cratestack#650): a trailing/blank line in CHANGELOG_FILES_OVERRIDE is skipped, not a bogus empty path"
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

# A trailing newline AND an embedded blank line, deliberately, to cover both
# shapes an override string could take.
run_capture_multi "$ROOT_FIXTURE"$'\n\n'"$PKG_FIXTURE"$'\n' "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  test_pass "changelog-seed succeeded despite blank lines in the override"
else
  test_fail "changelog-seed failed on an override with blank lines: $REPLY_OUT"
fi

if echo "$REPLY_OUT" | grep -qE "^error: +not found"; then
  test_fail "Bogus empty-path error resurfaced ('error:  not found' with no filename)"
else
  test_pass "No bogus empty-path error"
fi

if grep -q "^## 0.9.9 (" "$ROOT_FIXTURE" && grep -q "^## 0.9.9 (" "$PKG_FIXTURE"; then
  test_pass "Both real paths in the override were still seeded correctly"
else
  test_fail "One or both real paths in the override were not seeded"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 20 (cratestack#650 — reviewer-caught silent-degradation risk): a
# renamed, emptied, or typo'd CHANGELOG_FILES_DEFAULT must not make
# changelog-check.sh pass vacuously. Bash's `"${ARR[@]}"` on an unset/empty
# array expands to zero elements under `set -u` — NOT an error (bash 4.4+,
# what GitHub runners ship) — so without an explicit guard, an empty
# declared set makes the check loop zero times and report "no unedited
# seeds" having checked nothing. THIS is the decisive test: it fails if the
# guard in changelog-check.sh is ever removed, because without it this
# scenario would exit 0, not the non-zero + named error asserted below.
test_header "Test 20 (cratestack#650): changelog-check refuses (not vacuously passes) when the declared set is empty"
TEST_DIR=$(mktemp -d)
EMPTY_SET_FIXTURE="$TEST_DIR/changelog-files-empty.sh"
cat > "$EMPTY_SET_FIXTURE" <<'FIXTURE'
CHANGELOG_FILES_DEFAULT=()
FIXTURE

run_capture_with_source "$EMPTY_SET_FIXTURE" "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -ne 0 ]; then
  test_pass "changelog-check exited non-zero on an empty declared set (did not pass vacuously)"
else
  test_fail "changelog-check exited 0 on an empty declared set — vacuous pass, checked nothing"
fi

if echo "$REPLY_OUT" | grep -qi "empty"; then
  test_pass "changelog-check named the problem (empty declared set) rather than failing silently/generically"
else
  test_fail "changelog-check's error output did not explain the empty-set problem: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 21 (cratestack#650): the same guard, in changelog-seed.sh — an empty
# declared set must refuse to seed rather than silently seeding zero files
# and reporting success.
test_header "Test 21 (cratestack#650): changelog-seed refuses (not vacuously succeeds) when the declared set is empty"
TEST_DIR=$(mktemp -d)
EMPTY_SET_FIXTURE="$TEST_DIR/changelog-files-empty.sh"
cat > "$EMPTY_SET_FIXTURE" <<'FIXTURE'
CHANGELOG_FILES_DEFAULT=()
FIXTURE

run_capture_with_source "$EMPTY_SET_FIXTURE" "$SEED_SCRIPT" 0.9.9
if [ "$REPLY_STATUS" -ne 0 ]; then
  test_pass "changelog-seed exited non-zero on an empty declared set (did not silently succeed)"
else
  test_fail "changelog-seed exited 0 on an empty declared set — silently seeded nothing and called it success"
fi

if echo "$REPLY_OUT" | grep -qi "empty"; then
  test_pass "changelog-seed named the problem (empty declared set) rather than failing silently/generically"
else
  test_fail "changelog-seed's error output did not explain the empty-set problem: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 22 (cratestack#650): the same guard also catches a
# CHANGELOG_FILES_OVERRIDE that resolves to zero files (e.g. blank lines
# only) — not just a broken CHANGELOG_FILES_DEFAULT. This is the path
# introduced by Test 19's fix (skipping blank lines when splitting the
# override) — proving that fix didn't just trade one silent-empty-set path
# for another.
test_header "Test 22 (cratestack#650): an all-blank-lines CHANGELOG_FILES_OVERRIDE is also caught by the empty-set guard"
run_capture_multi $'\n\n' "$CHECK_SCRIPT"
if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -qi "empty"; then
  test_pass "changelog-check refused and named the empty-set problem for an all-blank-lines override"
else
  test_fail "changelog-check should have refused with a named empty-set error: status=$REPLY_STATUS out=$REPLY_OUT"
fi

# Tests 23-26 (cratestack#688): the invariant this issue exists to
# implement — "after the seed script runs, every declared changelog file
# has exactly one '## Unreleased' heading, empty, immediately above the
# newest dated section" — on all three paths, plus the multi-file case.
#
# These assert on EXACT file content (whole-file diff, or exact line
# ranges for the parts that are deterministic), not `grep` substring
# presence, per the issue's own warning: the conversion branch streams its
# body via `sed` specifically to avoid a `$(...)`-captured variable
# silently eating a trailing blank line, and inserting a heading into that
# same reconstruction is exactly where a heading-glued-to-content
# regression would pass a substring `grep` and still be wrong. A `diff`
# against literal expected content is the only assertion that class of bug
# can't slip past.
TODAY="$(date -u +"%Y-%m-%d")"

# Test 23 (cratestack#688 — path 1: prose present): the WHOLE resulting
# file is deterministic here (no commit-log-derived content is involved
# when there's real prose to carry forward), so this asserts the exact,
# complete file content — not just the presence of a heading.
test_header "Test 23 (cratestack#688): fresh empty '## Unreleased' re-seeded above the dated section, prose-carry path — exact content"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## Unreleased

### Real content

Prose body line.

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

EXPECTED_FILE="$TEST_DIR/expected.md"
cat > "$EXPECTED_FILE" <<FIXTURE
# Changelog

## Unreleased

## 9.9.9 ($TODAY)

### Real content

Prose body line.

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 9.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  if diff -u "$EXPECTED_FILE" "$SANDBOX_CHANGELOG"; then
    test_pass "Exact file content matches: one empty '## Unreleased' immediately above the new dated section, prose carried forward untouched"
  else
    test_fail "File content does not exactly match the expected reconstruction (see diff above)"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 24 (cratestack#688 — path 2: '## Unreleased' present but empty): the
# seeded section's body is derived from this repo's actual commit log, so
# it isn't reproducible as a static fixture — but the part at risk of the
# whitespace bug (the fresh heading immediately followed by the dated
# heading) is fully deterministic and asserted here as an exact multi-line
# match, along with an exact match of the untouched tail below the seeded
# section.
test_header "Test 24 (cratestack#688): fresh empty '## Unreleased' re-seeded above the dated section, empty-Unreleased fallback path — exact content at the seam"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## Unreleased

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 9.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  head_actual=$(sed -n '1,5p' "$SANDBOX_CHANGELOG")
  head_expected="# Changelog

## Unreleased

## 9.9.9 ($TODAY)"
  if [ "$head_actual" = "$head_expected" ]; then
    test_pass "Exact leading content: '# Changelog', blank, '## Unreleased', blank, '## 9.9.9 (date)' — no glued lines"
  else
    test_fail "Leading content does not exactly match. Expected:
$head_expected
Got:
$head_actual"
  fi

  tail_actual=$(tail -n 3 "$SANDBOX_CHANGELOG")
  tail_expected="## 1.2.3 (2026-01-01)

Older release notes here."
  if [ "$tail_actual" = "$tail_expected" ]; then
    test_pass "Exact trailing content: the pre-existing older section is untouched, byte-for-byte"
  else
    test_fail "Trailing content does not exactly match. Expected:
$tail_expected
Got:
$tail_actual"
  fi

  unreleased_count=$(grep -c "^## Unreleased$" "$SANDBOX_CHANGELOG" || true)
  if [ "$unreleased_count" -eq 1 ]; then
    test_pass "Exactly one '## Unreleased' heading in the file"
  else
    test_fail "Expected exactly one '## Unreleased' heading, found $unreleased_count"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 25 (cratestack#688 — path 3: no '## Unreleased' heading at all): same
# seam-exactness approach as Test 24 — the commit-derived body isn't a
# static fixture, but the heading insertion is.
test_header "Test 25 (cratestack#688): '## Unreleased' heading now present after seeding, no-heading fallback path — exact content at the seam"
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 1.2.3 (2026-01-01)

Older release notes here.
FIXTURE

run_capture "$SEED_SCRIPT" 9.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  head_actual=$(sed -n '1,5p' "$SANDBOX_CHANGELOG")
  head_expected="# Changelog

## Unreleased

## 9.9.9 ($TODAY)"
  if [ "$head_actual" = "$head_expected" ]; then
    test_pass "Exact leading content: '# Changelog', blank, '## Unreleased', blank, '## 9.9.9 (date)' — no glued lines"
  else
    test_fail "Leading content does not exactly match. Expected:
$head_expected
Got:
$head_actual"
  fi

  tail_actual=$(tail -n 3 "$SANDBOX_CHANGELOG")
  tail_expected="## 1.2.3 (2026-01-01)

Older release notes here."
  if [ "$tail_actual" = "$tail_expected" ]; then
    test_pass "Exact trailing content: the pre-existing older section is untouched, byte-for-byte"
  else
    test_fail "Trailing content does not exactly match. Expected:
$tail_expected
Got:
$tail_actual"
  fi

  unreleased_count=$(grep -c "^## Unreleased$" "$SANDBOX_CHANGELOG" || true)
  if [ "$unreleased_count" -eq 1 ]; then
    test_pass "Exactly one '## Unreleased' heading in the file (previously there was none at all — the bug this issue fixes)"
  else
    test_fail "Expected exactly one '## Unreleased' heading, found $unreleased_count"
  fi
else
  test_fail "changelog-seed script failed: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Test 26 (cratestack#688): the invariant holds on EVERY file in a
# multi-file set, not just a single file — the acceptance criterion
# explicitly calls out that the Dart package changelog goes through the
# identical cycle. Two sandbox fixtures, both with prose to carry (fully
# deterministic, so both get an exact whole-file diff), wired together via
# CHANGELOG_FILES_OVERRIDE.
test_header "Test 26 (cratestack#688): the fresh '## Unreleased' invariant holds across every file in a multi-file set"
TEST_DIR=$(mktemp -d)
ROOT_FIXTURE="$TEST_DIR/root/CHANGELOG.md"
PKG_FIXTURE="$TEST_DIR/pkg/CHANGELOG.md"
mkdir -p "$TEST_DIR/root" "$TEST_DIR/pkg"
cat > "$ROOT_FIXTURE" <<'FIXTURE'
# Changelog

## Unreleased

### Root package prose

Root prose body.

## 1.2.3 (2026-01-01)

Older root release notes.
FIXTURE
cat > "$PKG_FIXTURE" <<'FIXTURE'
## Unreleased

### Dart package prose

Dart package prose body.

## 0.8.3 (2026-01-01)

Older package release notes.
FIXTURE

EXPECTED_ROOT="$TEST_DIR/expected-root.md"
cat > "$EXPECTED_ROOT" <<FIXTURE
# Changelog

## Unreleased

## 9.9.9 ($TODAY)

### Root package prose

Root prose body.

## 1.2.3 (2026-01-01)

Older root release notes.
FIXTURE
EXPECTED_PKG="$TEST_DIR/expected-pkg.md"
cat > "$EXPECTED_PKG" <<FIXTURE
## Unreleased

## 9.9.9 ($TODAY)

### Dart package prose

Dart package prose body.

## 0.8.3 (2026-01-01)

Older package release notes.
FIXTURE

run_capture_multi "$ROOT_FIXTURE"$'\n'"$PKG_FIXTURE" "$SEED_SCRIPT" 9.9.9
if [ "$REPLY_STATUS" -eq 0 ]; then
  if diff -u "$EXPECTED_ROOT" "$ROOT_FIXTURE"; then
    test_pass "Root fixture: exact content matches, fresh '## Unreleased' present"
  else
    test_fail "Root fixture content does not exactly match the expected reconstruction (see diff above)"
  fi

  if diff -u "$EXPECTED_PKG" "$PKG_FIXTURE"; then
    test_pass "Package fixture: exact content matches, fresh '## Unreleased' present (the Dart package changelog goes through the identical cycle)"
  else
    test_fail "Package fixture content does not exactly match the expected reconstruction (see diff above)"
  fi
else
  test_fail "changelog-seed failed on the multi-file set: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

# Helper for Tests 27-29 (cratestack#713): a disposable git fixture repo
# shaped like the piece of the real repo's layout this feature cares about —
# a package directory ("pkg"), two directories standing in for the extra
# Rust crates a widened no-op scope reaches into ("vendor-a", "vendor-b" —
# analogous to crates/cratestack-client-flutter and crates/cratestack-cbor-
# wasm/crates/cratestack-codec-cbor in production), and one directory that
# is deliberately OUTSIDE any declared scope ("unrelated" — analogous to
# crates/cratestack-core, excluded per changelog-files.sh's comment). Tagged
# at v1.0.0 with zero post-tag commits; callers add their own post-tag
# commit(s) to exercise a specific scenario, same GIT_DIR/GIT_WORK_TREE seam
# Test 5 (cratestack#670) uses.
setup_noop_fixture_repo() {
  NOOP_GIT_DIR=$(mktemp -d)
  git init -q -b main "$NOOP_GIT_DIR"
  git -C "$NOOP_GIT_DIR" config user.email "changelog-seed-tests@example.invalid"
  git -C "$NOOP_GIT_DIR" config user.name "changelog-seed-tests"
  git -C "$NOOP_GIT_DIR" config commit.gpgsign false
  mkdir -p "$NOOP_GIT_DIR/pkg" "$NOOP_GIT_DIR/vendor-a" "$NOOP_GIT_DIR/vendor-b" "$NOOP_GIT_DIR/unrelated"
  echo init > "$NOOP_GIT_DIR/pkg/f.txt"
  echo init > "$NOOP_GIT_DIR/vendor-a/f.txt"
  echo init > "$NOOP_GIT_DIR/vendor-b/f.txt"
  echo init > "$NOOP_GIT_DIR/unrelated/f.txt"
  git -C "$NOOP_GIT_DIR" add -A
  git -C "$NOOP_GIT_DIR" commit -q -m "feat: pre-tag commit that must not appear in the seed"
  git -C "$NOOP_GIT_DIR" tag v1.0.0
}

cleanup_noop_fixture_repo() {
  rm -rf "$NOOP_GIT_DIR"
  NOOP_GIT_DIR=""
}

# Test 27 (cratestack#713): zero non-bump commits anywhere in the declared
# no-op scope (package directory + the extra vendoring directories) writes
# the standard, stable "No functional changes" wording instead of the
# marker+commit-list placeholder — no manual edit needed, and the
# `changelog (no unedited seeds)` gate passes immediately. This is
# Acceptance Criterion 1.
test_header "Test 27 (cratestack#713): zero commits in the declared no-op scope writes the standard no-op wording, no manual edit needed, gate passes"
setup_noop_fixture_repo
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE

NOOP_SOURCE="$TEST_DIR/changelog-files-noop.sh"
cat > "$NOOP_SOURCE" <<EOF
CHANGELOG_FILES_DEFAULT=("$SANDBOX_CHANGELOG")
declare -A CHANGELOG_NOOP_SCOPES=(
  ["$SANDBOX_CHANGELOG"]="$NOOP_GIT_DIR/pkg $NOOP_GIT_DIR/vendor-a $NOOP_GIT_DIR/vendor-b"
)
EOF

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$NOOP_SOURCE" CHANGELOG_FILE="$SANDBOX_CHANGELOG" GIT_DIR="$NOOP_GIT_DIR/.git" GIT_WORK_TREE="$NOOP_GIT_DIR" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_fail "Placeholder marker written even though the declared no-op scope had zero non-bump commits"
  else
    test_pass "No placeholder marker written — no-op scope was empty"
  fi

  if grep -q "^- No functional changes. Version kept in lockstep with the CrateStack$" "$SANDBOX_CHANGELOG" \
    && grep -q "^  workspace, which every published CrateStack artifact shares.$" "$SANDBOX_CHANGELOG"; then
    test_pass "Standard 'No functional changes' wording present, byte-matching the convention stable across 0.8.3/0.8.6/0.8.9/0.8.10"
  else
    test_fail "Standard no-op wording missing (or not byte-matching) from the seeded section"
  fi

  REPLY_STATUS=0
  REPLY_OUT=$(CHANGELOG_FILE="$SANDBOX_CHANGELOG" "$CHECK_SCRIPT" 2>&1) || REPLY_STATUS=$?
  if [ "$REPLY_STATUS" -eq 0 ]; then
    test_pass "changelog-check passes immediately, with no manual edit (Acceptance Criterion 1)"
  else
    test_fail "changelog-check should have passed without a manual edit: $REPLY_OUT"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
cleanup_noop_fixture_repo

# Test 28 (cratestack#713 — THE DECISIVE TEST, Acceptance Criterion 2): a
# real, non-bump commit reaching the no-op scope ONLY through one of the
# EXTRA directories — never the package's own directory — still writes the
# placeholder, and the gate still fails until a human writes prose. This is
# the concrete scenario the widened scope exists for: v0.8.6 shipped a real
# cratestack-codec-cbor fix baked into cratestack_cbor's vendored binaries
# while dart-packages/cratestack_cbor/ itself carried zero commits. If this
# test instead put the commit under "pkg/", it would prove nothing new —
# the pre-#713, package-directory-only proxy already caught that case.
test_header "Test 28 (cratestack#713 — DECISIVE): a change reaching only an extra scope directory (not the package's own) still blocks the gate"
setup_noop_fixture_repo
echo changed > "$NOOP_GIT_DIR/vendor-a/f.txt"
git -C "$NOOP_GIT_DIR" add -A
git -C "$NOOP_GIT_DIR" commit -q -m "fix: a real change reaching the vendored artifact, not the package directory"

TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE

NOOP_SOURCE="$TEST_DIR/changelog-files-noop.sh"
cat > "$NOOP_SOURCE" <<EOF
CHANGELOG_FILES_DEFAULT=("$SANDBOX_CHANGELOG")
declare -A CHANGELOG_NOOP_SCOPES=(
  ["$SANDBOX_CHANGELOG"]="$NOOP_GIT_DIR/pkg $NOOP_GIT_DIR/vendor-a $NOOP_GIT_DIR/vendor-b"
)
EOF

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$NOOP_SOURCE" CHANGELOG_FILE="$SANDBOX_CHANGELOG" GIT_DIR="$NOOP_GIT_DIR/.git" GIT_WORK_TREE="$NOOP_GIT_DIR" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_pass "Placeholder correctly written — the change reached the widened scope even though it never touched the package's own directory"
  else
    test_fail "No placeholder written — a real change in the widened scope was missed (exactly the gap #713's widened scope exists to close)"
  fi

  if grep -q "No functional changes. Version kept in lockstep" "$SANDBOX_CHANGELOG"; then
    test_fail "No-op wording incorrectly written for a range with a real change in scope — this would hide a real change"
  else
    test_pass "No-op wording correctly withheld"
  fi

  REPLY_STATUS=0
  REPLY_OUT=$(CHANGELOG_FILE="$SANDBOX_CHANGELOG" "$CHECK_SCRIPT" 2>&1) || REPLY_STATUS=$?
  if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "contain unedited seed"; then
    test_pass "changelog-check still fails until a human writes prose (Acceptance Criterion 2 — the decisive one)"
  else
    test_fail "changelog-check should have failed on the unedited placeholder: status=$REPLY_STATUS out=$REPLY_OUT"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
cleanup_noop_fixture_repo

# Test 29 (cratestack#713): a changelog file that is explicitly named in
# CHANGELOG_NOOP_EXEMPT — standing in for the root CHANGELOG.md, the only
# file exempted there in production — never takes the no-op fallback, even
# with zero commits anywhere. This is Acceptance Criterion 3 ("the root
# CHANGELOG.md path is unaffected"), exercised structurally rather than by
# trusting the production declaration alone. Deliberately uses the
# EXEMPT list, not an omitted CHANGELOG_NOOP_SCOPES entry with no
# exemption either — that latter shape is what Test 31 below exists to
# catch (the coverage guard), and would fail before reaching the seed
# logic this test wants to exercise.
test_header "Test 29 (cratestack#713): an exempted file (standing in for the root CHANGELOG.md) never takes the no-op fallback"
setup_noop_fixture_repo
TEST_DIR=$(mktemp -d)
SANDBOX_CHANGELOG="$TEST_DIR/CHANGELOG.md"
cat > "$SANDBOX_CHANGELOG" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE

NOOP_SOURCE="$TEST_DIR/changelog-files-noop.sh"
cat > "$NOOP_SOURCE" <<EOF
CHANGELOG_FILES_DEFAULT=("$SANDBOX_CHANGELOG")
declare -A CHANGELOG_NOOP_SCOPES=()
CHANGELOG_NOOP_EXEMPT=("$SANDBOX_CHANGELOG")
EOF

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$NOOP_SOURCE" CHANGELOG_FILE="$SANDBOX_CHANGELOG" GIT_DIR="$NOOP_GIT_DIR/.git" GIT_WORK_TREE="$NOOP_GIT_DIR" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -eq 0 ]; then
  if grep -q "TODO: edit this section from the seed below" "$SANDBOX_CHANGELOG"; then
    test_pass "Exempted file still takes the ordinary placeholder path — the no-op fallback never fires for a file named in CHANGELOG_NOOP_EXEMPT"
  else
    test_fail "Exempted file unexpectedly took the no-op fallback"
  fi
else
  test_fail "changelog-seed failed to run: $REPLY_OUT"
fi

rm -rf "$TEST_DIR"
cleanup_noop_fixture_repo

# Test 30 (cratestack#713, mirrors Test 18): the PRODUCTION
# CHANGELOG_NOOP_SCOPES actually declares cratestack_cbor's widened scope —
# its own package directory plus every Rust crate directory that produces
# its vendored binaries — and never declares one for the root CHANGELOG.md.
# Also (coordinator follow-up): dart-packages/cratestack_annotations and
# dart-packages/cratestack_builder are declared too, and EVERY file in the
# production CHANGELOG_FILES_DEFAULT is covered by either a scope or
# CHANGELOG_NOOP_EXEMPT — the same invariant the coverage guard in
# changelog-seed.sh enforces at runtime, asserted here directly against the
# declared data. A static assertion against the real, tracked
# changelog-files.sh, no mutation of any tracked file.
test_header "Test 30 (cratestack#713): the production CHANGELOG_NOOP_SCOPES covers all three Dart packages, never the root changelog, and leaves nothing uncovered"
declare -A CHANGELOG_NOOP_SCOPES=()
CHANGELOG_NOOP_EXEMPT=()
# shellcheck source=/dev/null
source "$REPO_ROOT/.ci/changelog-files.sh"

cbor_scope="${CHANGELOG_NOOP_SCOPES["dart-packages/cratestack_cbor/CHANGELOG.md"]:-}"
if [ -n "$cbor_scope" ]; then
  test_pass "cratestack_cbor's changelog is declared in CHANGELOG_NOOP_SCOPES"
else
  test_fail "cratestack_cbor's changelog is missing from CHANGELOG_NOOP_SCOPES"
fi

all_present=true
for expected in "dart-packages/cratestack_cbor" "crates/cratestack-client-flutter" "crates/cratestack-cbor-wasm" "crates/cratestack-codec-cbor"; do
  case " $cbor_scope " in
    *" $expected "*) ;;
    *)
      all_present=false
      echo "  missing from declared scope: $expected"
      ;;
  esac
done
if [ "$all_present" = "true" ]; then
  test_pass "Scope includes the package directory and every Rust crate directory that produces its vendored binaries"
else
  test_fail "Scope is missing one or more expected directories (see above)"
fi

if [ -z "${CHANGELOG_NOOP_SCOPES["CHANGELOG.md"]:-}" ]; then
  test_pass "Root CHANGELOG.md is never a key in CHANGELOG_NOOP_SCOPES (Acceptance Criterion 3: unaffected)"
else
  test_fail "Root CHANGELOG.md unexpectedly has a declared no-op scope"
fi

annotations_scope="${CHANGELOG_NOOP_SCOPES["dart-packages/cratestack_annotations/CHANGELOG.md"]:-}"
if [ -n "$annotations_scope" ] && case " $annotations_scope " in *" dart-packages/cratestack_annotations "*) true ;; *) false ;; esac; then
  test_pass "cratestack_annotations's changelog is declared in CHANGELOG_NOOP_SCOPES, scoped to its own directory"
else
  test_fail "cratestack_annotations's changelog is missing from CHANGELOG_NOOP_SCOPES, or missing its own directory from the scope"
fi

builder_scope="${CHANGELOG_NOOP_SCOPES["dart-packages/cratestack_builder/CHANGELOG.md"]:-}"
if [ -n "$builder_scope" ] && case " $builder_scope " in *" dart-packages/cratestack_builder "*) true ;; *) false ;; esac; then
  test_pass "cratestack_builder's changelog is declared in CHANGELOG_NOOP_SCOPES, scoped to its own directory"
else
  test_fail "cratestack_builder's changelog is missing from CHANGELOG_NOOP_SCOPES, or missing its own directory from the scope"
fi

# The coverage invariant itself: every file actually declared in
# CHANGELOG_FILES_DEFAULT has either a scope or an exemption. This is the
# same check changelog-seed.sh's guard performs at runtime; asserted here
# too so a broken declaration is visible from this test alone, not only
# when the seed script happens to run.
fully_covered=true
for declared_file in "${CHANGELOG_FILES_DEFAULT[@]}"; do
  is_exempt=false
  for exempt_file in "${CHANGELOG_NOOP_EXEMPT[@]}"; do
    [ "$exempt_file" = "$declared_file" ] && is_exempt=true && break
  done
  if [ "$is_exempt" = "false" ] && [ -z "${CHANGELOG_NOOP_SCOPES[$declared_file]:-}" ]; then
    fully_covered=false
    echo "  uncovered: $declared_file"
  fi
done
if [ "$fully_covered" = "true" ]; then
  test_pass "Every file in the production CHANGELOG_FILES_DEFAULT has either a CHANGELOG_NOOP_SCOPES entry or a CHANGELOG_NOOP_EXEMPT name"
else
  test_fail "One or more declared changelogs are neither scoped nor exempted (see above) — changelog-seed.sh's coverage guard would refuse to run"
fi

# And the guard itself, exercised end to end against the REAL production
# CHANGELOG_FILES_SOURCE (unset here, so it defaults to the real, tracked
# .ci/changelog-files.sh) — but with CHANGELOG_FILE pointed at a throwaway
# sandbox path, so nothing real is ever written. The coverage guard checks
# CHANGELOG_FILES_DEFAULT from CHANGELOG_FILES_SOURCE, not the CHANGELOG_FILE
# override, so this exercises the real declared data's coverage without
# touching any tracked file — a stronger check than the hand-rolled
# `fully_covered` loop above, because it runs the actual guard code path in
# changelog-seed.sh, not a re-derivation of its logic that could itself
# drift out of sync with the real implementation.
GUARD_PROBE_DIR=$(mktemp -d)
GUARD_PROBE_FILE="$GUARD_PROBE_DIR/throwaway.md"
printf '# Changelog\n' > "$GUARD_PROBE_FILE"
REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILE="$GUARD_PROBE_FILE" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?
if echo "$REPLY_OUT" | grep -q "no CHANGELOG_NOOP_SCOPES entry"; then
  test_fail "The real, production declared set trips the coverage guard: $REPLY_OUT"
else
  test_pass "The real, production declared set does not trip the coverage guard (exercised via the real guard code path, sandboxed write target)"
fi
rm -rf "$GUARD_PROBE_DIR"

# Test 31 (cratestack#713 — coordinator follow-up, DECISIVE for the
# coverage guard): a declared changelog with NEITHER a CHANGELOG_NOOP_SCOPES
# entry NOR a CHANGELOG_NOOP_EXEMPT name must fail loudly, not silently
# revert to "just never benefits from the no-op mechanism" — which is
# exactly what happened to dart-packages/cratestack_annotations and
# dart-packages/cratestack_builder when they were added to
# CHANGELOG_FILES_DEFAULT in #714 with no scope of their own. A fourth
# fixture file, deliberately given neither, proves the guard actually
# guards; a fifth, otherwise-identical fixture with the same file properly
# exempted proves the guard doesn't false-positive on a fully-covered set.
test_header "Test 31 (cratestack#713 — DECISIVE): a declared changelog with no scope and no exemption fails loudly, not silently"
TEST_DIR=$(mktemp -d)
COVERED_FIXTURE="$TEST_DIR/covered/CHANGELOG.md"
UNCOVERED_FIXTURE="$TEST_DIR/uncovered/CHANGELOG.md"
mkdir -p "$TEST_DIR/covered" "$TEST_DIR/uncovered"
cat > "$COVERED_FIXTURE" <<'FIXTURE'
# Changelog

## 0.7.8 (2026-08-08)

Some previously released, already-edited prose.
FIXTURE
cp "$COVERED_FIXTURE" "$UNCOVERED_FIXTURE"

# The uncovered fixture is declared, but has neither a CHANGELOG_NOOP_SCOPES
# entry nor a CHANGELOG_NOOP_EXEMPT name — the omission this guard exists to
# catch.
UNCOVERED_SOURCE="$TEST_DIR/changelog-files-uncovered.sh"
cat > "$UNCOVERED_SOURCE" <<EOF
CHANGELOG_FILES_DEFAULT=("$COVERED_FIXTURE" "$UNCOVERED_FIXTURE")
declare -A CHANGELOG_NOOP_SCOPES=(
  ["$COVERED_FIXTURE"]="$COVERED_FIXTURE"
)
CHANGELOG_NOOP_EXEMPT=()
EOF

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$UNCOVERED_SOURCE" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if [ "$REPLY_STATUS" -ne 0 ] && echo "$REPLY_OUT" | grep -q "no CHANGELOG_NOOP_SCOPES entry"; then
  test_pass "changelog-seed refused (RED) — an omitted scope/exemption is caught before any file is written"
else
  test_fail "changelog-seed should have refused with a named coverage error: status=$REPLY_STATUS out=$REPLY_OUT"
fi

if echo "$REPLY_OUT" | grep -qF "$UNCOVERED_FIXTURE"; then
  test_pass "The error names the specific uncovered file"
else
  test_fail "The error did not name the uncovered file: $REPLY_OUT"
fi

if grep -q "^## 0.9.9 (" "$COVERED_FIXTURE"; then
  test_fail "The covered fixture was written even though the set as a whole was refused — not atomic"
else
  test_pass "Nothing was written — the coverage guard runs before any file is touched"
fi

# Now the inverse — the SAME two files, but the previously-uncovered one is
# properly exempted (mirroring how CHANGELOG.md is exempted in production).
# This must NOT trip the coverage guard (it may still fail later for
# unrelated reasons, so this only asserts the coverage-guard error text is
# absent, not that the whole run succeeds).
COVERED_SOURCE="$TEST_DIR/changelog-files-covered.sh"
cat > "$COVERED_SOURCE" <<EOF
CHANGELOG_FILES_DEFAULT=("$COVERED_FIXTURE" "$UNCOVERED_FIXTURE")
declare -A CHANGELOG_NOOP_SCOPES=(
  ["$COVERED_FIXTURE"]="$COVERED_FIXTURE"
)
CHANGELOG_NOOP_EXEMPT=("$UNCOVERED_FIXTURE")
EOF

REPLY_STATUS=0
REPLY_OUT=$(CHANGELOG_FILES_SOURCE="$COVERED_SOURCE" "$SEED_SCRIPT" 0.9.9 2>&1) || REPLY_STATUS=$?

if echo "$REPLY_OUT" | grep -q "no CHANGELOG_NOOP_SCOPES entry"; then
  test_fail "Coverage guard fired even though the previously-uncovered file is now exempted: $REPLY_OUT"
else
  test_pass "Coverage guard does not false-positive once every declared file is scoped or exempted (GREEN)"
fi

rm -rf "$TEST_DIR"
TEST_DIR=""

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
