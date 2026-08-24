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

# Every changelog the release pipeline is responsible for is declared once,
# centrally, in changelog-files.sh — see that file for why. Overridable
# (absolute path) so the self-tests can point this script at an alternate
# declaration file — e.g. a fixture with a deliberately empty set, to prove
# the guard below actually guards — without touching the real, tracked one.
CHANGELOG_FILES_SOURCE="${CHANGELOG_FILES_SOURCE:-$PROJECT_ROOT/.ci/changelog-files.sh}"
# shellcheck source=.ci/changelog-files.sh
source "$CHANGELOG_FILES_SOURCE"

# cratestack#713: re-declared (idempotently — `declare -A` on an already
# associative variable does not clear it) so this script never dies with
# "unbound variable" reading ${CHANGELOG_NOOP_SCOPES[...]} below when
# CHANGELOG_FILES_SOURCE points at a fixture that doesn't declare this map
# at all (every self-test fixture that predates cratestack#713, and any
# future one that isn't exercising this feature specifically).
declare -A CHANGELOG_NOOP_SCOPES

# cratestack#713 (coverage guard): every file in CHANGELOG_FILES_DEFAULT
# must be accounted for by the no-op mechanism — either it has a
# CHANGELOG_NOOP_SCOPES entry, or it's named in CHANGELOG_NOOP_EXEMPT
# (declared or not; "${CHANGELOG_NOOP_EXEMPT[@]}" on a wholly unset array
# expands to zero elements under `set -u`, same as CHANGELOG_FILES_DEFAULT
# elsewhere in this script — no defensive re-declare needed the way the
# associative CHANGELOG_NOOP_SCOPES above needs one). A file with neither
# used to fail silently: it just never benefited from the no-op mechanism
# and kept needing the identical hand-edit on every release forever —
# exactly what happened to dart-packages/cratestack_annotations and
# dart-packages/cratestack_builder, added to CHANGELOG_FILES_DEFAULT in
# #714 with no CHANGELOG_NOOP_SCOPES entry of their own. This makes that
# omission fail loudly instead, the next time a changelog is declared here.
#
# Deliberately checked against CHANGELOG_FILES_DEFAULT itself — the array
# actually sourced from CHANGELOG_FILES_SOURCE — not the resolved
# CHANGELOG_FILES below. That keeps this guard about the DECLARED set
# (what changelog-files.sh says the release pipeline owns), independent of
# a test using CHANGELOG_FILE/CHANGELOG_FILES_OVERRIDE to redirect what
# gets WRITTEN for an unrelated scenario — Tests 1-26 in
# changelog-seed-tests.sh do exactly that against arbitrary sandbox paths
# with no scope or exemption of their own, and must not trip this guard on
# the production declared set's behalf. A test exercising THIS guard
# instead points CHANGELOG_FILES_SOURCE at its own fixture, so both
# CHANGELOG_FILES_DEFAULT and CHANGELOG_NOOP_SCOPES/CHANGELOG_NOOP_EXEMPT
# come from that fixture together.
uncovered_changelogs=()
for declared_file in "${CHANGELOG_FILES_DEFAULT[@]}"; do
  is_exempt=false
  for exempt_file in "${CHANGELOG_NOOP_EXEMPT[@]}"; do
    if [ "$exempt_file" = "$declared_file" ]; then
      is_exempt=true
      break
    fi
  done
  if [ "$is_exempt" = "false" ] && [ -z "${CHANGELOG_NOOP_SCOPES[$declared_file]:-}" ]; then
    uncovered_changelogs+=("$declared_file")
  fi
done
if [ "${#uncovered_changelogs[@]}" -gt 0 ]; then
  echo "error: the following declared changelog(s) have no CHANGELOG_NOOP_SCOPES entry and are not named in CHANGELOG_NOOP_EXEMPT — add one or the other in .ci/changelog-files.sh (cratestack#713):" >&2
  for declared_file in "${uncovered_changelogs[@]}"; do
    echo "  - $declared_file" >&2
  done
  exit 1
fi

# Overridable (absolute path) so tests can target a single sandbox copy
# instead of the declared set above, while git further below still walks
# this repo's real history. This is the degenerate single-file case: when
# set, it replaces the whole declared set with just the one path. Existing
# behavior, unchanged.
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
# zero times and this script would report success having seeded nothing —
# and downstream, changelog-check.sh would report "no unedited seeds"
# having checked nothing, and prepare-release.yml's `git add` would stage
# zero changelogs, shipping a release with NO changelog update at all
# (worse than the original #650 bug). Checked here on the FINAL resolved
# CHANGELOG_FILES, not just CHANGELOG_FILES_DEFAULT, so this also catches a
# CHANGELOG_FILES_OVERRIDE that resolves to nothing (e.g. blank lines only)
# — not just a broken declared-set file.
#
# Deliberately guarded in each consumer rather than inside
# changelog-files.sh itself: that file's own header says it "only declares
# data" and is "meant to be sourced, not executed directly" — adding
# control flow there would break that contract for all three consumers
# (this script, changelog-check.sh, and prepare-release.yml's `git add`
# step), which each need a differently-worded, differently-scoped error
# anyway (this one talks about seeding; changelog-check.sh's about
# checking; the workflow's about staging).
if [ "${#CHANGELOG_FILES[@]}" -eq 0 ]; then
  echo "error: the declared changelog set is empty — nothing to seed. Check CHANGELOG_FILES_DEFAULT in .ci/changelog-files.sh (or the CHANGELOG_FILE / CHANGELOG_FILES_OVERRIDE env override in effect, if any)." >&2
  exit 1
fi

# Verify every changelog in the set exists, and that none of them already
# has a section for this version, before writing to ANY of them. This makes
# THIS validation pass atomic: a file that's missing or already has the
# section is caught before any writes happen, for any file in the set. It
# does NOT make the write loop below atomic against a failure mid-write
# (e.g. a permissions error or a full disk on the second file) — a write
# failure there can still leave some files seeded and others untouched. Making
# the writes themselves atomic (stage every file to a temp path, then move
# all of them) is more machinery than this script currently carries; a
# release script that fails loudly and dirty on a write error is an
# acceptable, honestly-documented limitation.
for file in "${CHANGELOG_FILES[@]}"; do
  if [ ! -f "$file" ]; then
    echo "error: $file not found" >&2
    exit 1
  fi

  if grep -q "^## $VERSION " "$file"; then
    echo "error: $file already contains a section for $VERSION" >&2
    exit 1
  fi
done

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

# Write the section into every declared changelog. The commit range and the
# grouped-by-type new_section computed above are shared across all of them —
# they describe the same repo history regardless of which file is being
# written — only the per-file "is there an '## Unreleased' section to carry
# forward" decision below varies file to file.
for CHANGELOG_FILE in "${CHANGELOG_FILES[@]}"; do

# cratestack#531: if CHANGELOG.md already has a "## Unreleased" section (the
# convention: individual PRs add narrative prose there as they land, between
# releases), convert THAT section into the new dated release section instead
# of inserting a fresh seed above it. Without this, prose written by every
# PR merged since the last release got stranded under a stale "## Unreleased"
# heading buried below the release section, while the release section itself
# held only the placeholder seed — precisely how v0.7.12 shipped with the
# seed unedited: the human step "rewrite the seed into prose" was asking for
# work already done, it just needed folding up. The seed (marker +
# placeholder + grouped commits) is used only when there is genuinely
# nothing to carry forward: no "## Unreleased" heading at all, or one
# present but empty (whitespace only) — the state right after a release,
# before any PR has added anything.
unreleased_line=$(grep -n '^## Unreleased$' "$CHANGELOG_FILE" | head -1 | cut -d: -f1 || true)

unreleased_body=""
unreleased_end_line=""
if [ -n "$unreleased_line" ]; then
  next_heading_line=$(awk -v start="$unreleased_line" 'NR > start && /^## / { print NR; exit }' "$CHANGELOG_FILE")
  if [ -n "$next_heading_line" ]; then
    unreleased_end_line=$((next_heading_line - 1))
  else
    unreleased_end_line=$(wc -l < "$CHANGELOG_FILE")
  fi
  content_start_line=$((unreleased_line + 1))
  if [ "$unreleased_end_line" -ge "$content_start_line" ]; then
    unreleased_body=$(sed -n "${content_start_line},${unreleased_end_line}p" "$CHANGELOG_FILE")
  fi
fi

# Whitespace-only counts as "nothing to carry" — same bar as an absent section.
has_prose_to_carry=false
if [ -n "$(printf '%s' "$unreleased_body" | tr -d '[:space:]')" ]; then
  has_prose_to_carry=true
fi

# cratestack#713: decide up front whether the eventual fallback (used by
# both branches below that have nothing to carry forward) should write the
# established "no functional changes" wording instead of the marker+
# commit-list placeholder. Computed even when there IS real prose to carry
# (the branch immediately below) — hand-written prose always wins over this
# auto-generated wording, so the result here is simply unused in that case,
# not special-cased around it.
#
# The scope is deliberately NOT just this file's own package directory —
# see changelog-files.sh's CHANGELOG_NOOP_SCOPES comment for the concrete,
# shipped counter-example (v0.8.6) that a narrower scope got wrong. A file
# not present in CHANGELOG_NOOP_SCOPES (the root CHANGELOG.md, always —
# acceptance criterion: its path takes the conversion branch above, never
# this fallback) leaves noop_scope empty and this block is a no-op.
noop_section=""
noop_scope="${CHANGELOG_NOOP_SCOPES[$CHANGELOG_FILE]:-}"
if [ -n "$noop_scope" ]; then
  # Deliberately unquoted: $noop_scope is a space-separated list of
  # pathspecs (see changelog-files.sh) and word-splitting it into multiple
  # `git log -- <path> <path> ...` arguments is the point.
  # shellcheck disable=SC2086
  noop_commit_count=$(git log "$range" --pretty=%s -- $noop_scope | wc -l | tr -d '[:space:]')
  if [ "$noop_commit_count" -eq 0 ]; then
    # `printf -v`, not `noop_section=$(printf ...)`: command substitution
    # strips ALL trailing newlines (the same reason the '## Unreleased'
    # conversion branch above streams via `sed`/`printf` instead of a
    # captured variable) — this needs its OWN trailing blank line preserved
    # so the section that follows (the next tail/heading) isn't glued
    # directly under "...shares.", matching every other section boundary in
    # this file (see e.g. v0.8.10's shipped entry: a blank line separates
    # "...shares." from the next "## " heading).
    printf -v noop_section '## %s (%s)\n\n- No functional changes. Version kept in lockstep with the CrateStack\n  workspace, which every published CrateStack artifact shares.\n\n' "$VERSION" "$today"
  fi
fi

# Whichever fallback branch below fires, it writes this — the no-op
# wording when the check above found zero non-bump commits anywhere in
# scope, otherwise the standard marker+commit-list placeholder, unchanged.
section_to_write="$new_section"
if [ -n "$noop_section" ]; then
  section_to_write="$noop_section"
fi

tmp_file=$(mktemp)
trap "rm -f '$tmp_file'" EXIT

if [ -n "$unreleased_line" ] && [ "$has_prose_to_carry" = "true" ]; then
  # Carry the existing prose forward under the new dated heading, in place.
  # No seed marker/placeholder is written — there is real content already.
  # cratestack#688: a stale "## Unreleased" heading no longer "remains"
  # anywhere in the file afterward, but a FRESH, empty one is deliberately
  # re-emitted immediately above the new dated heading, in the same
  # reconstruction, below. Without this, every release consumes the
  # heading and never puts it back — the next contributor finds no
  # "## Unreleased" section to write under, so they either misfile their
  # entry under the newest *released* section (#672, #680, #686) or, worse,
  # the seed's placeholder-detection fallback fires again because nobody
  # wrote real prose anywhere the tooling recognizes (precisely how v0.7.12
  # and v0.8.6 shipped with the seed unedited). Re-seeding the heading here
  # closes that loop.
  # The body is streamed straight from the file via `sed`, not through a
  # `$(...)`-captured variable — command substitution strips ALL trailing
  # newlines, which would silently eat a genuine trailing blank line
  # separating the carried prose from whatever section follows it. The
  # fresh "## Unreleased" heading below is emitted the same way, via
  # `printf` straight into the same output stream, for the same reason:
  # inserting it through a captured variable would risk the exact class of
  # whitespace bug this comment already warns about, right where a heading
  # would end up glued to the line after it.
  before_line=$((unreleased_line - 1))
  after_line=$((unreleased_end_line + 1))
  {
    if [ "$before_line" -gt 0 ]; then
      head -n "$before_line" "$CHANGELOG_FILE"
    fi
    printf '## Unreleased\n\n'
    printf '## %s (%s)\n' "$VERSION" "$today"
    sed -n "${content_start_line},${unreleased_end_line}p" "$CHANGELOG_FILE"
    total_lines=$(wc -l < "$CHANGELOG_FILE")
    if [ "$after_line" -le "$total_lines" ]; then
      tail -n "+${after_line}" "$CHANGELOG_FILE"
    fi
  } > "$tmp_file"
  mv "$tmp_file" "$CHANGELOG_FILE"

  echo "$CHANGELOG_FILE: converted existing '## Unreleased' section into '## $VERSION ($today)' — prose carried forward, fresh empty '## Unreleased' re-seeded above it"
  echo "  Last tag: ${last_tag:-none}"
  echo "  Commit range: $range"
  echo "  Computed from commit: $head_sha"
elif [ -n "$unreleased_line" ]; then
  # "## Unreleased" exists but is empty (nothing landed since the last
  # release yet) — replace the heading + its empty body with the full
  # seeded section. cratestack#688: a fresh, empty "## Unreleased" heading
  # is re-emitted immediately above the seeded section in the same pass —
  # this branch used to leave the file with NO "## Unreleased" heading at
  # all afterward, which is the exact state that made #672/#680/#686
  # misfile their entries under the newest released section instead. This
  # is additive only: the seeded section itself (marker, placeholder,
  # grouped commits) is unchanged from before.
  before_line=$((unreleased_line - 1))
  after_line=$((unreleased_end_line + 1))
  {
    if [ "$before_line" -gt 0 ]; then
      head -n "$before_line" "$CHANGELOG_FILE"
    fi
    printf '## Unreleased\n\n'
    printf '%s' "$section_to_write"
    total_lines=$(wc -l < "$CHANGELOG_FILE")
    if [ "$after_line" -le "$total_lines" ]; then
      tail -n "+${after_line}" "$CHANGELOG_FILE"
    fi
  } > "$tmp_file"
  mv "$tmp_file" "$CHANGELOG_FILE"

  if [ -n "$noop_section" ]; then
    echo "$CHANGELOG_FILE: no non-bump commits in the declared no-op scope since ${last_tag:-the start of history} — wrote the standard 'No functional changes' entry for $VERSION (cratestack#713), no manual edit needed — '## Unreleased' was present but empty, replaced in place, fresh empty '## Unreleased' re-seeded above it"
  else
    echo "$CHANGELOG_FILE: seeded with section for $VERSION (marker: $section_marker) — '## Unreleased' was present but empty, replaced in place, fresh empty '## Unreleased' re-seeded above it"
  fi
  echo "  Last tag: ${last_tag:-none}"
  echo "  Commit range: $range"
  echo "  Computed from commit: $head_sha"
  echo "  Today's date: $today"
else
  # No "## Unreleased" heading at all — original behavior: insert above the
  # current newest entry, but below leading front matter (the "# Changelog"
  # H1 + blurb) — prepending at byte 0 would push that title out of the top
  # and bury it mid-file on every run. Insert right before the first
  # existing "## " heading instead, preserving everything above it untouched.
  #
  # cratestack#688: a fresh, empty "## Unreleased" heading is re-emitted
  # immediately above the seeded section here too. This branch is the
  # purest case of the bug this issue closes: a file with no
  # "## Unreleased" heading at all is exactly the state every earlier
  # release left behind, with nowhere obvious for the next contributor to
  # write. The seeded section itself is otherwise unchanged.
  insert_line=$(grep -n '^## ' "$CHANGELOG_FILE" | head -1 | cut -d: -f1 || true)

  if [ -n "$insert_line" ]; then
    before_line=$((insert_line - 1))
    {
      if [ "$before_line" -gt 0 ]; then
        head -n "$before_line" "$CHANGELOG_FILE"
      fi
      printf '## Unreleased\n\n'
      printf '%s' "$section_to_write"
      tail -n "+${insert_line}" "$CHANGELOG_FILE"
    } > "$tmp_file"
  else
    # No existing "## " section heading found (e.g. a brand-new changelog) —
    # append the new section after whatever front matter/content is there.
    {
      cat "$CHANGELOG_FILE"
      printf '## Unreleased\n\n'
      printf '%s' "$section_to_write"
    } > "$tmp_file"
  fi
  mv "$tmp_file" "$CHANGELOG_FILE"

  if [ -n "$noop_section" ]; then
    echo "$CHANGELOG_FILE: no non-bump commits in the declared no-op scope since ${last_tag:-the start of history} — wrote the standard 'No functional changes' entry for $VERSION (cratestack#713), no manual edit needed, fresh empty '## Unreleased' re-seeded above it"
  else
    echo "$CHANGELOG_FILE: seeded with section for $VERSION (marker: $section_marker), fresh empty '## Unreleased' re-seeded above it"
  fi
  echo "  Last tag: ${last_tag:-none}"
  echo "  Commit range: $range"
  echo "  Computed from commit: $head_sha"
  echo "  Today's date: $today"
fi

done
