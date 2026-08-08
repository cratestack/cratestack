#!/usr/bin/env bash
# Tests for changelog-seed.sh and changelog-check.sh
#
# Usage: changelog-seed-tests.sh
#
# These tests verify that:
# 1. changelog-seed creates a new section with the correct format
# 2. changelog-seed refuses to overwrite existing sections
# 3. changelog-check detects unedited seeds
# 4. changelog-check passes when seeds are edited

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PASS_COUNT=0
FAIL_COUNT=0
TEST_DIR=""

test_pass() {
  echo -e "${GREEN}✓${NC} $1"
  ((PASS_COUNT++))
}

test_fail() {
  echo -e "${RED}✗${NC} $1"
  ((FAIL_COUNT++))
}

test_header() {
  echo -e "\n${YELLOW}=== $1 ===${NC}"
}

setup_test() {
  TEST_DIR=$(mktemp -d)
  trap "rm -rf '$TEST_DIR'" EXIT
  cd "$TEST_DIR"
  # Copy CHANGELOG.md to test directory
  cp "$PROJECT_ROOT/CHANGELOG.md" .
}

cleanup_test() {
  cd "$PROJECT_ROOT"
  rm -rf "$TEST_DIR"
}

# Test 1: Seed creates section with correct format
test_header "Test 1: changelog-seed creates a new section"
setup_test

if "$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 2>&1 | grep -q "seeded CHANGELOG.md"; then
  # Check that the section was added
  if grep -q "^## 0.9.9 (" CHANGELOG.md; then
    test_pass "New section added with correct heading format"
  else
    test_fail "Section heading not found"
  fi

  # Check that the TODO marker is present
  if grep -q "TODO: edit this section from the seed below" CHANGELOG.md; then
    test_pass "TODO marker present in new section"
  else
    test_fail "TODO marker not found"
  fi

  # Check that it's positioned above the old content
  if head -1 CHANGELOG.md | grep -q "^## 0.9.9"; then
    test_pass "New section positioned at top of file"
  else
    test_fail "New section not at top of file"
  fi
else
  test_fail "changelog-seed script failed"
fi

cleanup_test

# Test 2: Seed refuses to overwrite existing section
test_header "Test 2: changelog-seed refuses to overwrite existing section"
setup_test

# Create initial seed
"$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 >/dev/null 2>&1

# Try to seed the same version again
if "$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 2>&1 | grep -q "already contains a section"; then
  test_pass "Correctly refuses to overwrite existing section"
else
  test_fail "Should have refused to overwrite existing section"
fi

cleanup_test

# Test 3: changelog-check detects unedited seeds
test_header "Test 3: changelog-check detects unedited seeds"
setup_test

# Create a seed
"$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 >/dev/null 2>&1

# Run check - should fail because seed is unedited
if ! "$PROJECT_ROOT/.ci/changelog-check.sh" 2>&1 | grep -q "contains unedited seed"; then
  test_fail "changelog-check should have detected unedited seed"
else
  test_pass "changelog-check correctly detected unedited seed"
fi

cleanup_test

# Test 4: changelog-check passes when seed is edited
test_header "Test 4: changelog-check passes when seed is edited"
setup_test

# Create a seed
"$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 >/dev/null 2>&1

# Remove the TODO marker to simulate editing
sed -i '/<!-- TODO: edit this section from the seed below -->/d' CHANGELOG.md

# Run check - should pass now
if "$PROJECT_ROOT/.ci/changelog-check.sh" 2>&1; then
  test_pass "changelog-check passes after TODO marker removed"
else
  test_fail "changelog-check should pass after seed is edited"
fi

cleanup_test

# Test 5: Seed includes commits grouped by type
test_header "Test 5: changelog-seed groups commits by conventional-commit type"
setup_test

if "$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 >/dev/null 2>&1; then
  # Check for at least one type section
  local type_sections=0
  type_sections=$((type_sections + $(grep -c "^#### " CHANGELOG.md || echo 0)))

  if [ $type_sections -gt 0 ]; then
    test_pass "Seed includes type groupings (found $type_sections sections)"
  else
    test_fail "No type groupings found in seed"
  fi
else
  test_fail "changelog-seed failed to run"
fi

cleanup_test

# Test 6: Seed formats date correctly (YYYY-MM-DD)
test_header "Test 6: changelog-seed formats date correctly"
setup_test

if "$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9.9 >/dev/null 2>&1; then
  # Check that date format is correct (YYYY-MM-DD)
  if grep -E "^## 0.9.9 \([0-9]{4}-[0-9]{2}-[0-9]{2}\)" CHANGELOG.md >/dev/null; then
    test_pass "Date formatted correctly (YYYY-MM-DD)"
  else
    test_fail "Date format is incorrect"
  fi
else
  test_fail "changelog-seed failed to run"
fi

cleanup_test

# Test 7: Seed refuses invalid version format
test_header "Test 7: changelog-seed refuses invalid version format"
setup_test

if "$PROJECT_ROOT/.ci/changelog-seed.sh" v0.9.9 2>&1 | grep -q "VERSION must be X.Y.Z format"; then
  test_pass "Correctly refuses version with 'v' prefix"
else
  test_fail "Should refuse version with 'v' prefix"
fi

if "$PROJECT_ROOT/.ci/changelog-seed.sh" 0.9 2>&1 | grep -q "VERSION must be X.Y.Z format"; then
  test_pass "Correctly refuses incomplete version"
else
  test_fail "Should refuse incomplete version"
fi

cleanup_test

# Summary
echo -e "\n${YELLOW}=== Test Summary ===${NC}"
TOTAL=$((PASS_COUNT + FAIL_COUNT))
echo "Passed: $PASS_COUNT/$TOTAL"
echo "Failed: $FAIL_COUNT/$TOTAL"

if [ $FAIL_COUNT -eq 0 ]; then
  echo -e "\n${GREEN}All tests passed!${NC}"
  exit 0
else
  echo -e "\n${RED}Some tests failed.${NC}"
  exit 1
fi
