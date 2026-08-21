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
# centrally, in changelog-files.sh — see that file for why.
# shellcheck source=.ci/changelog-files.sh
source "$PROJECT_ROOT/.ci/changelog-files.sh"

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
  mapfile -t CHANGELOG_FILES <<< "$CHANGELOG_FILES_OVERRIDE"
else
  CHANGELOG_FILES=("${CHANGELOG_FILES_DEFAULT[@]}")
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
