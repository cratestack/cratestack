#!/usr/bin/env bash
# Docs & skills parity declaration gate.
#
# WHAT THIS CHECKS, PRECISELY: that a PR which ANNOUNCES a user-facing change
# also SAYS what happened to the two companion repos that document that
# change for humans (cratestack/cratestack-docs) and for agents
# (cratestack/cratestack-skills). It does NOT — and cannot, from inside this
# repository — verify that those companion changes were actually made. It
# enforces an honest declaration, not the parity itself. Naming that limit
# here so nobody later reads a green run as "docs and skills are in sync".
#
# WHY THE TRIGGER IS THE CHANGELOG, not a path allowlist: "did this PR touch
# crates/cratestack-macros/" fires on nearly every PR in this repo, including
# pure refactors with no surface change at all — a gate that fires on
# everything gets rubber-stamped and then stops meaning anything. A "### "
# entry a PR adds under "## Unreleased" is this repo's own existing,
# maintained signal for "a human needs to be told about this", so it is the
# right trigger and costs no new metadata. The diff-parsing machinery is
# reused wholesale from changelog-placement-check.sh (cratestack#739) rather
# than reimplemented.
#
# Consequence, by design: a PR that ships a user-facing feature and writes NO
# changelog entry passes this gate. That gap is real and deliberate — closing
# it means a generic "every PR needs a changelog entry" rule, which this repo
# has considered and not adopted (see changelog-check.sh's header).
#
# Usage:
#   PR_BODY="$(gh pr view --json body -q .body)" ./.ci/parity-declaration-check.sh
#
# In GitHub Actions the workflow supplies PR_BODY from
# github.event.pull_request.body.
#
# Exits 0 when there is nothing to declare or the declaration is present and
# well-formed, 1 when a declaration is required and missing/malformed, 2 on
# an environment error (which is NOT treated as a pass).

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# shellcheck source=.ci/changelog-placement-check.sh
source "$PROJECT_ROOT/.ci/changelog-placement-check.sh"

# Only the root changelog. The dart-packages changelogs describe package
# releases, not framework surface, and have no docs/skills counterpart.
PARITY_CHANGELOG="${PARITY_CHANGELOG:-CHANGELOG.md}"

resolve_placement_refs

if [ -z "$PLACEMENT_BASE_REF" ]; then
  echo "warning: no base ref resolvable (checked \$CHANGELOG_CHECK_BASE_REF, origin/\$GITHUB_BASE_REF, origin/main) — cannot tell which changelog entries this branch adds, so the parity declaration gate is skipping. This is not a pass." >&2
  exit 0
fi

if [ ! -f "$PARITY_CHANGELOG" ]; then
  echo "error: $PARITY_CHANGELOG not found (cwd: $PWD)" >&2
  exit 2
fi

# Counts "### " lines the diff adds that land under "## Unreleased"
# specifically — not every added "### " line. The distinction is what keeps
# a release bump out of this gate: `changelog-seed.sh` promotes
# "## Unreleased" into a fresh "## X.Y.Z (date)" heading, so the entries a
# bump "adds" sit under a dated heading the same diff created. Those are
# already-declared entries moving, not new claims about the surface, and
# asking prepare-release to fill in a parity block would be noise that
# teaches people to write "n/a" without reading. Same nearest-preceding-
# heading walk changelog-placement-check.sh does, applied for the opposite
# purpose.
_parity_count_unreleased_entries() {
  local file="$1" head_ref="$2"
  PARITY_UNRELEASED_ENTRIES=0
  [ "${#PLACEMENT_ADDED_ENTRIES[@]}" -eq 0 ] && return 0

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

  local entry_line best best_text i hl
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
    if [ "$best_text" = "## Unreleased" ]; then
      PARITY_UNRELEASED_ENTRIES=$((PARITY_UNRELEASED_ENTRIES + 1))
    fi
  done

  # Explicit, and load-bearing under `set -e`: without it the function's
  # status is the last loop test's, so a final entry that is NOT under
  # "## Unreleased" returns 1 and kills the script before it can report
  # "nothing to declare". Caught by sandbox cases 7 and 8.
  return 0
}

_placement_parse_diff "$PARITY_CHANGELOG" "$PLACEMENT_BASE_REF" "$PLACEMENT_HEAD_REF"
_parity_count_unreleased_entries "$PARITY_CHANGELOG" "$PLACEMENT_HEAD_REF"

added_entries=$PARITY_UNRELEASED_ENTRIES
if [ "$added_entries" -eq 0 ]; then
  echo "parity: no \"## Unreleased\" entry added in ${PLACEMENT_BASE_REF}..${PLACEMENT_HEAD_REF} — nothing to declare."
  exit 0
fi

# A PR body is the only place the declaration can live. Resolve it from the
# environment first (how CI passes it), then fall back to `gh` for a local
# run, and skip loudly if neither is available rather than inventing a pass.
body="${PR_BODY:-}"
if [ -z "$body" ] && command -v gh > /dev/null 2>&1; then
  body="$(gh pr view --json body -q .body 2>/dev/null || true)"
fi

if [ -z "$body" ]; then
  if [ "${GITHUB_ACTIONS:-}" = "true" ] && [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ]; then
    echo "error: this PR adds $added_entries changelog entr(y/ies) but PR_BODY is empty. The workflow must pass github.event.pull_request.body." >&2
    exit 2
  fi
  echo "warning: $added_entries changelog entr(y/ies) added, but no PR body is available here (set PR_BODY, or run inside a checkout with \`gh\` authenticated and an open PR). Skipping — this is not a pass." >&2
  exit 0
fi

# The declaration. Deliberately two independent lines rather than one
# combined "docs+skills" checkbox: the two repos drift independently, and a
# single checkbox lets one of them be silently forgotten behind the other.
#
# A bare "n/a" is rejected. "n/a" with a reason is accepted, because plenty
# of real changelog entries (a CI fix, a dependency bump, an internal
# refactor worth announcing) genuinely need no companion change — but the
# reason is what makes that a decision rather than an omission.
missing=()
for field in docs skills; do
  line="$(printf '%s\n' "$body" | grep -iE "^[[:space:]]*[-*][[:space:]]*${field}:" | head -n 1 || true)"
  if [ -z "$line" ]; then
    missing+=("$field: line absent")
    continue
  fi
  value="$(printf '%s\n' "$line" | sed -E "s/^[[:space:]]*[-*][[:space:]]*${field}:[[:space:]]*//I")"
  value="$(printf '%s\n' "$value" | sed -E 's/[[:space:]]+$//')"
  if [ -z "$value" ]; then
    missing+=("$field: line present but empty")
    continue
  fi
  # Bare n/a, with or without punctuation, and nothing after it.
  if printf '%s\n' "$value" | grep -qiE '^(n/?a|none|not applicable)[[:punct:]]*$'; then
    missing+=("$field: \"$value\" with no reason — say why no companion change is needed")
  fi
done

if [ ${#missing[@]} -gt 0 ]; then
  cat >&2 <<EOF
error: this PR adds $added_entries changelog entr(y/ies) under "## Unreleased",
so it must declare what happened to the docs and skills companions.

Problems:
$(printf '  - %s\n' "${missing[@]}")

Add this to the PR body (section 9 of the pull request template):

  ## 9. Docs & Skills Parity

  - docs: https://github.com/cratestack/cratestack-docs/pull/NN
  - skills: https://github.com/cratestack/cratestack-skills/pull/NN

Either value may be "n/a — <reason>" when no companion change is warranted,
e.g. "n/a — internal CI fix, no user-facing surface".

This gate checks the declaration only. It cannot see the other repositories,
so a green run here does not mean docs and skills are in sync.
EOF
  exit 1
fi

echo "parity: $added_entries changelog entr(y/ies) added; docs and skills both declared."
exit 0
