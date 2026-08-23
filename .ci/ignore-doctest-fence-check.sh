#!/usr/bin/env bash
# Regression guard for cratestack#683: ```ignore is the wrong fence for
# illustrative (non-compilable) doc examples in this repo.
#
# Background: on edition-2024 crates, `cargo test --doc -- --ignored`
# merges doctests and reports every ```ignore-fenced block as passing
# WITHOUT compiling it (verified in #683 against the pinned toolchain).
# That makes `just test-ci-ignored-report` / the `tests-ignored-report` CI
# job blind to anything fenced ```ignore. Forcing them to compile
# (`--merge-doctests=no`) was considered and rejected: every ```ignore
# doctest in this repo at the time of #683 turned out to be illustrative
# pseudocode (elided structs, free variables with no scope, a JSON env-var
# value, a nonexistent schema path) that was never meant to compile —
# forcing compilation would make the job permanently red for zero real
# defects, which is worse than the blindness it "fixes".
#
# The actual fix (generalizing cratestack#611's `8fb373d`, which did this
# for the single macro-generated `invoke_with_db` example): illustrative
# content belongs in a ```text fence, not ```ignore. Rustdoc only ever
# schedules a fenced block as a doctest when it believes the block is
# Rust; ```text is never a doctest candidate, under any flag combination,
# in any merge mode — so there is nothing for the ignored-sweep to be
# blind about. This guard keeps that convention from silently regressing:
# a NEW ```ignore fence opened around illustrative content would recreate
# exactly the vacuous-pass hole #683 reports.
#
# What this does NOT cover: genuinely-real-but-skipped ```ignore doctests
# (real, would-compile Rust that's skipped for a structural reason, e.g.
# `crates/cratestack-rusqlite/src/opfs.rs`'s OPFS-bootstrap example, which
# only compiles under `--target wasm32-unknown-unknown` and is invisible
# to any doctest run on this repo's native-target CI). Those are
# deliberately exempted below (KNOWN_REAL_IGNORE_FENCES) rather than
# converted, since converting them to ```text would be equally dishonest
# in the other direction — they DO describe real, compiling code, just
# not on the target this sweep runs against. A new entry here should
# come with the same justification: name the reason compilation is
# structurally impossible under this repo's doctest sweep, not merely
# "slow" or "needs setup" (those belong in a `#[ignore]`d unit test, which
# this guard does not touch).
#
# Detects only an actual fence-OPENING line: exactly three backticks
# (a four-or-more backtick run is CommonMark's escape for talking about a
# fence without opening one, and this file itself — plus
# crates/cratestack-macros/src/procedure/tests.rs and
# .../instrument/invoke_with_db.rs, both of which discuss ```ignore in
# prose — rely on that distinction) followed by an info string whose
# words include exactly `ignore`. Mirrors
# `crates/cratestack-macros/src/procedure/tests.rs`'s
# `opens_fence_with_attribute` (the Rust-side regression guard for the
# single case #611 already fixed); this is the repo-wide, cross-crate
# counterpart for every hand-written doc comment.
#
# Run locally via `just verify-ignore-doctest-fences`. CI runs it as the
# `ignore-doctest-fence` job in `.github/workflows/ci.yml`.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# file:line pairs for fences already triaged as genuinely real-but-skipped
# (see the header above). A grep match at exactly one of these locations is
# not a failure; a grep match anywhere else is.
KNOWN_REAL_IGNORE_FENCES=(
  "crates/cratestack-rusqlite/src/opfs.rs:19"
)

is_known() {
  local candidate="$1"
  local entry
  for entry in "${KNOWN_REAL_IGNORE_FENCES[@]}"; do
    if [ "$entry" = "$candidate" ]; then
      return 0
    fi
  done
  return 1
}

# Matches a genuine fence-open line: optional leading whitespace, a `///`
# or `//!` doc-comment marker, then EXACTLY three backticks (not a
# four-plus run — that boundary is what makes this precise rather than a
# substring guess, matching the Rust-side guard's own reasoning) with
# `ignore` as one of the info-string words.
FENCE_PATTERN='^[[:space:]]*//[/!][[:space:]]*```ignore([[:space:],]|$)'

unexpected=()
while IFS= read -r match; do
  [ -z "$match" ] && continue
  file="${match%%:*}"
  line="${match#*:}"
  line="${line%%:*}"
  candidate="$file:$line"
  if ! is_known "$candidate"; then
    unexpected+=("$match")
  fi
done < <(grep -RnE "$FENCE_PATTERN" --include="*.rs" crates/ || true)

if [ "${#unexpected[@]}" -gt 0 ]; then
  echo "error: new \`\`\`ignore-fenced doctest(s) found outside the known real-but-skipped set:" >&2
  for m in "${unexpected[@]}"; do
    echo "  - $m" >&2
  done
  echo "" >&2
  echo "\`\`\`ignore is not the right fence for illustrative/non-compiling examples in this repo" >&2
  echo "(cratestack#683) — use \`\`\`text instead, which rustdoc never schedules as a doctest" >&2
  echo "under any flag combination. If this example is genuine, would-compile Rust that is" >&2
  echo "structurally skipped for a real reason (not merely slow/needs-setup — that belongs in a" >&2
  echo "#[ignore]d unit test instead), add its file:line to KNOWN_REAL_IGNORE_FENCES in" >&2
  echo "$(basename "$0") with a comment naming the reason." >&2
  exit 1
fi

# Guard against the allowlist itself going stale: every KNOWN_REAL_IGNORE_FENCES
# entry must correspond to an actual ```ignore fence still present at that
# exact location — an entry that doesn't match anything real is either a
# fence that was since converted (the entry should have been deleted) or a
# location that never opened a fence at all, both of which would silently
# widen the guard's blind spot exactly the way #683 reports.
stale=()
for entry in "${KNOWN_REAL_IGNORE_FENCES[@]}"; do
  file="${entry%%:*}"
  line="${entry#*:}"
  if [ ! -f "$file" ]; then
    stale+=("$entry (file not found)")
    continue
  fi
  actual_line="$(sed -n "${line}p" "$file" || true)"
  if ! printf '%s\n' "$actual_line" | grep -qE '```ignore([[:space:],]|$)'; then
    stale+=("$entry (no \`\`\`ignore fence at that line: \"$actual_line\")")
  fi
done

if [ "${#stale[@]}" -gt 0 ]; then
  echo "error: KNOWN_REAL_IGNORE_FENCES in $(basename "$0") has stale entries — fix or remove them:" >&2
  for s in "${stale[@]}"; do
    echo "  - $s" >&2
  done
  exit 1
fi

exit 0
