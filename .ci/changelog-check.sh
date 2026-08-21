#!/usr/bin/env bash
# Verify that no declared changelog contains an unedited seed.
#
# An unedited seed contains a TODO marker comment that indicates a human
# has not yet rewritten the auto-generated content into narrative prose.
# This check is load-bearing: without it, an unedited seed could reach
# `main` and the changelog would silently degrade from prose to a commit list,
# which is worse than an honest gap (it looks maintained, but isn't).
#
# Usage: changelog-check.sh
#
# Exits 0 if every declared changelog is edited (no TODO markers), 1 if any
# contain unedited seeds (naming which ones), 2 on error.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Every changelog the release pipeline is responsible for is declared once,
# centrally, in changelog-files.sh — see that file for why. Overridable
# (absolute path) so the self-tests can point this script at an alternate
# declaration file — e.g. a fixture with a deliberately empty set, to prove
# the guard below actually guards — without touching the real, tracked one.
CHANGELOG_FILES_SOURCE="${CHANGELOG_FILES_SOURCE:-$PROJECT_ROOT/.ci/changelog-files.sh}"
# shellcheck source=.ci/changelog-files.sh
source "$CHANGELOG_FILES_SOURCE"

# May be overridden (absolute path) so the test suite can point this script
# at a single isolated sandbox copy instead of the declared set above. This
# is the degenerate single-file case: when set, it replaces the whole
# declared set with just the one path. Existing behavior, unchanged.
#
# CHANGELOG_FILES_OVERRIDE is the multi-file counterpart: a newline-separated
# list of paths, used only by the self-tests to exercise the multi-file case
# against sandbox copies without touching the real, tracked paths declared
# in changelog-files.sh. CHANGELOG_FILE (singular) wins if both are set.
if [ -n "${CHANGELOG_FILE:-}" ]; then
  CHANGELOG_FILES=("$CHANGELOG_FILE")
elif [ -n "${CHANGELOG_FILES_OVERRIDE:-}" ]; then
  # A `while read` loop, not `mapfile ... <<<`: a here-string always appends
  # a trailing newline, and an override string that itself ends in a
  # newline (or contains a blank line) would otherwise produce an empty
  # array element — which later fails as a bare, unhelpful "error:  not
  # found" with no filename. Skipping blank lines here means a malformed
  # override fails on a real (missing) path instead.
  CHANGELOG_FILES=()
  while IFS= read -r line; do
    [ -n "$line" ] && CHANGELOG_FILES+=("$line")
  done <<< "$CHANGELOG_FILES_OVERRIDE"
else
  CHANGELOG_FILES=("${CHANGELOG_FILES_DEFAULT[@]}")
fi

# Guard against a silently empty resolved set. Bash's `"${ARR[@]}"` on an
# unset or empty array expands to zero elements under `set -u` — NOT an
# error (this changed in bash 4.4; the pre-4.4 unbound-variable-error
# behavior some people remember no longer applies, and GitHub runners ship
# bash 5.x). So if CHANGELOG_FILES_DEFAULT in changelog-files.sh is ever
# renamed, emptied, or typo'd, the for-loop below would silently iterate
# zero times and this script would report "no unedited seeds" and exit 0
# having checked NOTHING — a vacuous pass, exactly the "a check that cannot
# fail is not verification" failure this whole ticket exists to prevent.
# Checked here on the FINAL resolved CHANGELOG_FILES, not just
# CHANGELOG_FILES_DEFAULT, so this also catches a CHANGELOG_FILES_OVERRIDE
# that resolves to nothing (e.g. blank lines only) — not just a broken
# declared-set file.
#
# Deliberately guarded in each consumer rather than inside
# changelog-files.sh itself — see changelog-seed.sh's identical guard for
# the full rationale (that file's own header promises it "only declares
# data" and is "meant to be sourced, not executed directly").
if [ "${#CHANGELOG_FILES[@]}" -eq 0 ]; then
  echo "error: the declared changelog set is empty — nothing to check. Check CHANGELOG_FILES_DEFAULT in .ci/changelog-files.sh (or the CHANGELOG_FILE / CHANGELOG_FILES_OVERRIDE env override in effect, if any)." >&2
  exit 2
fi

# The marker that changelog-seed.sh leaves to indicate an unedited section
UNEDITED_MARKER="TODO: edit this section from the seed below"

unedited_files=()

for file in "${CHANGELOG_FILES[@]}"; do
  if [ ! -f "$file" ]; then
    echo "error: $file not found" >&2
    exit 2
  fi

  if grep -q "$UNEDITED_MARKER" "$file"; then
    unedited_files+=("$file")
  fi
done

if [ "${#unedited_files[@]}" -gt 0 ]; then
  echo "error: the following changelog(s) contain unedited seed(s) — rewrite into narrative prose before merging:" >&2
  for file in "${unedited_files[@]}"; do
    echo "  - $file" >&2
  done
  echo "  Marker: $UNEDITED_MARKER" >&2
  exit 1
fi

exit 0
