#!/usr/bin/env bash
# Verify that the CHANGELOG.md contains no unedited seeds.
#
# An unedited seed contains a TODO marker comment that indicates a human
# has not yet rewritten the auto-generated content into narrative prose.
# This check is load-bearing: without it, an unedited seed could reach
# `main` and the changelog would silently degrade from prose to a commit list,
# which is worse than an honest gap (it looks maintained, but isn't).
#
# Usage: changelog-check.sh
#
# Exits 0 if all sections are edited (no TODO markers), 1 if unedited seeds
# are found, 2 on error.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

CHANGELOG_FILE="CHANGELOG.md"

if [ ! -f "$CHANGELOG_FILE" ]; then
  echo "error: $CHANGELOG_FILE not found" >&2
  exit 2
fi

# The marker that changelog-seed.sh leaves to indicate an unedited section
UNEDITED_MARKER="TODO: edit this section from the seed below"

if grep -q "$UNEDITED_MARKER" "$CHANGELOG_FILE"; then
  echo "error: CHANGELOG.md contains unedited seed(s) — rewrite into narrative prose before merging" >&2
  echo "  Marker: $UNEDITED_MARKER" >&2
  exit 1
fi

exit 0
