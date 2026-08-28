# Detects a changelog entry ("### " heading) that a diff ADDS under a dated
# release section instead of under "## Unreleased" (cratestack#739).
#
# Meant to be sourced by changelog-check.sh, not executed directly — it only
# declares functions, the same contract changelog-files.sh documents for
# itself.
#
# DIFF-BASED, NOT A WHOLE-FILE SCAN: check_changelog_placement only looks at
# "### " lines a diff (BASE_REF -> HEAD_REF) ADDS to a file, never at "### "
# lines already sitting in the file before the diff. This is deliberate —
# cratestack#672/#680/#686 already landed entries under dated sections on
# `main`, and a whole-file scan would fail every future PR on THEIR account,
# not the PR's own (cratestack#739's Acceptance Criterion: only lines a PR
# adds are in scope).
#
# THE RELEASE-BUMP CARVE-OUT: a newly created dated section — the exact
# shape a release bump produces by promoting "## Unreleased"
# (changelog-seed.sh) — is NOT a violation even when the diff also adds
# "### " lines under it, as long as the diff itself ADDED the "## X.Y.Z
# (date)" heading line. Only an entry landing under a heading that already
# existed before the diff is a violation. This is the trap cratestack#739
# calls out as "the case most likely to break a naive implementation" —
# verified against both of this repo's own real release-bump shapes:
# `a3290a82` (v0.8.14 — prose carried forward from Unreleased, so the bump
# itself adds zero "### " lines: nothing to check) and `68a20ccb` (v0.8.12
# — the placeholder-seed fallback, which DOES add "### " lines, directly
# under the freshly-created dated heading: the carve-out actually fires).

# Parses `git diff --unified=0 BASE_REF HEAD_REF -- FILE` and populates two
# global arrays with 1-indexed line numbers IN THE HEAD_REF VERSION of the
# file: PLACEMENT_ADDED_HEADINGS ("## " lines the diff added) and
# PLACEMENT_ADDED_ENTRIES ("### " lines the diff added). --unified=0 keeps
# every hunk to changed lines only, so the running new-file line counter
# only needs to advance on '+' lines — a '-' line consumes no new-file line
# number and is otherwise ignored (a pure deletion needs no placement
# check: nothing was added).
_placement_parse_diff() {
  local file="$1" base_ref="$2" head_ref="$3"
  PLACEMENT_ADDED_HEADINGS=()
  PLACEMENT_ADDED_ENTRIES=()

  local diff_text
  diff_text=$(git diff --unified=0 --no-color "$base_ref" "$head_ref" -- "$file" 2>/dev/null || true)
  [ -z "$diff_text" ] && return 0

  local new_line=0 line content
  while IFS= read -r line; do
    if [[ "$line" =~ ^@@\ -[0-9]+(,[0-9]+)?\ \+([0-9]+)(,[0-9]+)?\ @@ ]]; then
      new_line="${BASH_REMATCH[2]}"
      continue
    fi
    # "+++ b/<file>" is the per-file diff header, not an added line.
    [[ "$line" == '+++'* ]] && continue
    if [[ "$line" == '+'* ]]; then
      content="${line:1}"
      if [[ "$content" == "### "* ]]; then
        PLACEMENT_ADDED_ENTRIES+=("$new_line")
      elif [[ "$content" == "## "* ]]; then
        PLACEMENT_ADDED_HEADINGS+=("$new_line")
      fi
      new_line=$((new_line + 1))
    fi
    # A '-' line consumes no new-file line number; left as-is.
  done <<< "$diff_text"
}

_placement_line_was_added() {
  local target="$1" ln
  for ln in "${PLACEMENT_ADDED_HEADINGS[@]}"; do
    [ "$ln" = "$target" ] && return 0
  done
  return 1
}

# check_changelog_placement FILE BASE_REF HEAD_REF
#
# Prints one "FILE:LINE: entry added under 'SECTION' ..." line per
# violation found and returns 1 if any were found, 0 otherwise (including
# "nothing to check": no diff for this file, or the diff added no "### "
# lines at all — the ordinary case for a PR that doesn't touch this
# changelog, cratestack#739's Acceptance Criterion 3).
check_changelog_placement() {
  local file="$1" base_ref="$2" head_ref="$3"

  _placement_parse_diff "$file" "$base_ref" "$head_ref"
  [ "${#PLACEMENT_ADDED_ENTRIES[@]}" -eq 0 ] && return 0

  # Headings as they exist in the HEAD_REF version of the file — the
  # version the diff actually produced, not BASE_REF's.
  local content_tmp
  content_tmp=$(mktemp)
  git show "$head_ref:$file" > "$content_tmp" 2>/dev/null || cat "$file" > "$content_tmp"

  local heading_lines=() heading_text=()
  local n=0 line
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    if [[ "$line" == "## "* ]]; then
      heading_lines+=("$n")
      heading_text+=("$line")
    fi
  done < "$content_tmp"
  rm -f "$content_tmp"

  local violations=0 entry_line best best_text i hl
  for entry_line in "${PLACEMENT_ADDED_ENTRIES[@]}"; do
    best=-1
    best_text=""
    for i in "${!heading_lines[@]}"; do
      hl="${heading_lines[$i]}"
      if [ "$hl" -le "$entry_line" ] && [ "$hl" -gt "$best" ]; then
        best="$hl"
        best_text="${heading_text[$i]}"
      fi
    done

    # OK: filed under "## Unreleased" — the normal, expected case.
    if [ "$best_text" = "## Unreleased" ]; then
      continue
    fi
    # OK: filed under a dated section, but that section heading itself was
    # ALSO added by this diff — a release bump legitimately promoting
    # "## Unreleased" into "## X.Y.Z (date)".
    if [ "$best" -ge 0 ] && _placement_line_was_added "$best"; then
      continue
    fi

    violations=$((violations + 1))
    if [ "$best" -ge 0 ]; then
      echo "$file:$entry_line: entry added under '${best_text}' (line $best) instead of '## Unreleased'"
    else
      echo "$file:$entry_line: entry added with no preceding '## ' section heading at all"
    fi
  done

  [ "$violations" -eq 0 ]
}

# Resolves the BASE_REF/HEAD_REF pair check_changelog_placement diffs
# between, into the globals PLACEMENT_BASE_REF/PLACEMENT_HEAD_REF.
# PLACEMENT_BASE_REF is left empty when no base can be resolved — callers
# must treat that as "skip the placement check, loudly" rather than
# crashing, since not every environment this script runs in (a shallow
# clone with no 'origin' remote, an ad hoc local run) has one available.
#
# Resolution order:
#   1. CHANGELOG_CHECK_BASE_REF / CHANGELOG_CHECK_HEAD_REF env overrides —
#      how the self-tests replay specific historical commit pairs (see
#      changelog-seed-tests.sh) and how CI could target a specific base
#      without relying on the 'origin' remote's default branch.
#   2. GITHUB_BASE_REF (set by GitHub Actions on pull_request events) —
#      the PR's actual target branch, in case it is ever not "main".
#   3. "origin/main" — the default for a push-to-main run, and the
#      fallback for any other context with a normal 'origin' remote.
resolve_placement_refs() {
  PLACEMENT_BASE_REF="${CHANGELOG_CHECK_BASE_REF:-}"
  PLACEMENT_HEAD_REF="${CHANGELOG_CHECK_HEAD_REF:-HEAD}"

  [ -n "$PLACEMENT_BASE_REF" ] && return 0

  if [ -n "${GITHUB_BASE_REF:-}" ] && git rev-parse --verify -q "origin/${GITHUB_BASE_REF}" > /dev/null; then
    PLACEMENT_BASE_REF="origin/${GITHUB_BASE_REF}"
  elif git rev-parse --verify -q "origin/main" > /dev/null; then
    PLACEMENT_BASE_REF="origin/main"
  fi
}
