#!/usr/bin/env bash
# Seed a CHANGELOG.md section for a release.
#
# Usage: changelog-seed.sh VERSION
#
# Writes a new `## X.Y.Z (YYYY-MM-DD)` section to CHANGELOG.md, positioned
# above the current newest entry, seeded from the commit range since the last
# release and grouped by conventional-commit type. The section is marked as
# unedited (a placeholder) so CI/tooling can detect it and block the merge
# until a human rewrites it into narrative prose.
#
# Non-negotiables:
#   1. Refuse to write if a section for that version already exists
#   2. An unedited seed must not reach main (CI must detect and reject it)
#
# This script mirrors the machinery that already extracts commit refs in
# prepare-release.yml:202, but outputs to a file instead of discarding it.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

usage() {
  echo "usage: ${BASH_SOURCE[0]} VERSION" >&2
  echo "example: ${BASH_SOURCE[0]} 0.7.9" >&2
  exit 2
}

if [ $# -ne 1 ]; then
  usage
fi

VERSION="$1"

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: VERSION must be X.Y.Z format (no leading 'v'), got: '$VERSION'" >&2
  exit 1
fi

# Overridable (absolute path) so tests can target a sandbox copy instead of
# the real CHANGELOG.md, while git below still walks this repo's real history.
CHANGELOG_FILE="${CHANGELOG_FILE:-CHANGELOG.md}"

if [ ! -f "$CHANGELOG_FILE" ]; then
  echo "error: $CHANGELOG_FILE not found" >&2
  exit 1
fi

# Refuse to write if a section for this version already exists
if grep -q "^## $VERSION " "$CHANGELOG_FILE"; then
  echo "error: CHANGELOG.md already contains a section for $VERSION" >&2
  exit 1
fi

# Compute the commit range since the last release tag.
# Deliberately NOT `git describe --tags --abbrev=0` — some past release tags
# in this repo were pushed pointing at a commit that never actually became an
# ancestor of main (a pre-existing mix of direct tag-pushes and squash-merged
# PRs), so `describe` can walk right past the real last release and land on a
# much older tag. The highest tag by version number is a more reliable "last
# release" signal than ancestry here.
last_tag=$(git tag --list 'v*' | sort -V | tail -1)
range="HEAD"
[ -n "$last_tag" ] && range="${last_tag}..HEAD"

# Extract commit messages and group by conventional-commit type.
# Format: type(scope): subject or just type: subject
# Groups commits that don't match the pattern under "other".
declare -A type_map

# Read all commits and group them
while IFS= read -r commit_subject; do
  type="other"
  subject="$commit_subject"

  # Try to match conventional-commit pattern: type[(scope)][!]: subject
  # Check first part: does it look like "type: " or "type(scope): " etc?
  first_colon_idx=-1
  if [[ "$commit_subject" == *":"* ]]; then
    first_colon_idx=$(echo "$commit_subject" | grep -o '^[^:]*' | wc -c)
    first_colon_idx=$((first_colon_idx - 1))
  fi

  if [ $first_colon_idx -gt 0 ]; then
    # Extract everything before the first colon
    prefix="${commit_subject:0:$first_colon_idx}"
    # The prefix should be "type" or "type(scope)" or "type!" or "type(scope)!"
    # Extract just the type (first word, letters only)
    if [[ "$prefix" =~ ^([a-z]+) ]]; then
      type="${BASH_REMATCH[1]}"
      # Extract subject: everything after ": "
      if [[ "$commit_subject" =~ :\ (.*)$ ]]; then
        subject="${BASH_REMATCH[1]}"
      fi
    fi
  fi

  # Initialize the type in the map if not already there
  if [ -z "${type_map[$type]:-}" ]; then
    type_map[$type]=""
  fi

  # Append to the type's list (using a special separator)
  # Use newline as separator since we're preserving order
  if [ -z "${type_map[$type]}" ]; then
    type_map[$type]="$subject"
  else
    type_map[$type]+=$'\n'"$subject"
  fi
done < <(git log "$range" --pretty=%s | sort -u || true)

# Canonical order for types (common conventional-commit types)
declare -a canonical_order=(feat fix docs chore refactor test ci perf build other)

# Generate the section content
today=$(date -u +"%Y-%m-%d")
section_marker="<!-- TODO: edit this section from the seed below -->"

# The commit the range was computed from, embedded in the persisted section
# (not just echoed to stdout) so a CHANGELOG.md diff — not just a workflow
# log, which can expire — shows what an omission would need to be checked
# against.
head_sha=$(git rev-parse HEAD)
range_marker="<!-- seeded from ${range} at ${head_sha} -->"

# Build the new section
new_section="## $VERSION ($today)

$section_marker
$range_marker

This is an auto-generated seed. Please rewrite into narrative prose describing
the changes in this release, grouped by concern. Refer to existing entries in
this file for the house prose style. Do not commit with this placeholder text.

### Changes

"

# Add commits grouped by type
added_any=false
for type in "${canonical_order[@]}"; do
  if [ -z "${type_map[$type]:-}" ]; then
    continue
  fi

  added_any=true

  # Convert the type label to title case for display
  case "$type" in
    feat) display_type="Features" ;;
    fix) display_type="Fixes" ;;
    docs) display_type="Documentation" ;;
    chore) display_type="Chores" ;;
    refactor) display_type="Refactoring" ;;
    test) display_type="Tests" ;;
    ci) display_type="CI" ;;
    perf) display_type="Performance" ;;
    build) display_type="Build" ;;
    other) display_type="Other" ;;
    *) display_type="${type^}" ;;
  esac

  new_section+="#### $display_type

"

  # Output each subject as a bullet point
  subjects="${type_map[$type]}"
  while IFS= read -r subject; do
    if [ -n "$subject" ]; then
      new_section+="- $subject
"
    fi
  done <<< "$subjects"

  new_section+="
"
done

if ! [ "$added_any" = "true" ]; then
  # No commits since last tag — just write an empty seed
  new_section+="- No changes since last release
"
fi

# Insert above the current newest entry, but below leading front matter (the
# "# Changelog" H1 + blurb) — prepending at byte 0 would push that title out
# of the top and bury it mid-file on every run. Insert right before the first
# existing "## " heading instead, preserving everything above it untouched.
insert_line=$(grep -n '^## ' "$CHANGELOG_FILE" | head -1 | cut -d: -f1 || true)

tmp_file=$(mktemp)
trap "rm -f '$tmp_file'" EXIT

if [ -n "$insert_line" ]; then
  before_line=$((insert_line - 1))
  {
    if [ "$before_line" -gt 0 ]; then
      head -n "$before_line" "$CHANGELOG_FILE"
    fi
    printf '%s' "$new_section"
    tail -n "+${insert_line}" "$CHANGELOG_FILE"
  } > "$tmp_file"
else
  # No existing "## " section heading found (e.g. a brand-new changelog) —
  # append the new section after whatever front matter/content is there.
  {
    cat "$CHANGELOG_FILE"
    printf '%s' "$new_section"
  } > "$tmp_file"
fi

mv "$tmp_file" "$CHANGELOG_FILE"

echo "seeded CHANGELOG.md with section for $VERSION (marker: $section_marker)"
echo "  Last tag: ${last_tag:-none}"
echo "  Commit range: $range"
echo "  Computed from commit: $head_sha"
echo "  Today's date: $today"
