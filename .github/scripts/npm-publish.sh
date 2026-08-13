#!/usr/bin/env bash
#
# Shared wrapper for every `npm publish` call in
# .github/workflows/release-cli.yml. Usage:
#
#   .github/scripts/npm-publish.sh <package-dir> [npm publish args...]
#
# It exists to make a release publish survive the two ways a bare
# `npm publish` fails this pipeline for reasons that are not actually
# "the publish is broken". Both were hit for real by v0.7.5
# (run 31094655414), which left the release half-landed: crates.io plus
# @cratestack/cli and @cratestack/ts-types at 0.7.5, everything else
# stuck at 0.7.4.
#
# 1. Sigstore transparency-log conflict (retry, with backoff).
#    npm's own internal retry to Rekor can race its own already-landed
#    tlog write; the retry then gets back `409 an equivalent entry
#    already exists in the transparency log`. sigstore-js defaults to
#    `fetchOnConflict: false`, so that benign duplicate surfaces as a
#    fatal TLOG_CREATE_ENTRY_ERROR and takes the whole publish down.
#    Upstream tracking issue: sigstore/sigstore-js#1708; the fix,
#    sigstore/sigstore-js#1709, was still OPEN and unmerged as of
#    2026-08-07 — so there is no patched npm version to pin to yet, and
#    a retry here is the only available mitigation. A *fresh*
#    `npm publish` re-signs with a new ephemeral Fulcio cert, which
#    clears the conflict; npm's own internal retry does not, because it
#    reuses the same signing material and fires with no real backoff.
#    Hence: whole-command retry, with a deliberate sleep between tries.
#    Revisit (and consider dropping) once #1709 ships in an npm release.
#
# 2. Version already published (skip, treat as success).
#    Re-running a release re-executes every publish job against the same
#    tag, including the ones that already succeeded. Without this, one
#    already-published package fails the job — and in
#    publish-npm-api-family, where the publishes run in a loop, the very
#    first already-published package aborts the script and the packages
#    that genuinely still need publishing are never even attempted.
#
#    Deliberately NOT implemented as an `npm view` pre-check: registry
#    read propagation lags the write path, so a read right after a
#    partial release can be stale in either direction. `npm publish`
#    already does this read itself before attempting the write, and if
#    that read is stale and the PUT is rejected server-side instead,
#    both paths produce the same error text. Matching that text after
#    the fact costs no extra round-trip and covers whichever path threw.
#    This mirrors what @napi-rs/cli's `napi prepublish` already does
#    internally for the per-platform subpackages it publishes.
#
# Anything else fails immediately and is NOT retried — this must never
# mask a genuine publish failure (bad auth, a missing Trusted Publisher
# entry, a broken tarball).

# No `set -e`: `out=$(...)` on a failing publish would abort before we
# could inspect why it failed, which is the entire point of this script.
set -uo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: npm-publish.sh <package-dir> [npm publish args...]" >&2
  exit 2
fi

pkg_dir=$1
shift

# Overridable so the retry path can be exercised without a minute of
# real sleeping; CI uses the defaults.
max_attempts=${NPM_PUBLISH_MAX_ATTEMPTS:-4}
backoff_seconds=${NPM_PUBLISH_BACKOFF_SECONDS:-20}
attempt=1

while true; do
  # Output is captured rather than streamed so it can be pattern-matched,
  # then echoed verbatim so the Actions log still shows the full npm
  # output (including the provenance/tarball notices) for every attempt.
  out=$(cd "$pkg_dir" && npm publish "$@" 2>&1)
  rc=$?
  printf '%s\n' "$out"

  if [ "$rc" -eq 0 ]; then
    exit 0
  fi

  # Checked before the tlog case on purpose: when a retried publish
  # actually landed on the previous attempt, the next attempt reports
  # "cannot publish over" rather than a tlog conflict. That is success.
  if printf '%s' "$out" | grep -qi 'cannot publish over the previously published version'; then
    echo "npm-publish: $pkg_dir is already published at this version — treating as success" >&2
    exit 0
  fi

  if printf '%s' "$out" | grep -qi 'TLOG_CREATE_ENTRY_ERROR\|transparency log'; then
    if [ "$attempt" -ge "$max_attempts" ]; then
      echo "npm-publish: $pkg_dir still failing on a sigstore tlog conflict after $attempt attempts — giving up" >&2
      exit "$rc"
    fi
    echo "npm-publish: $pkg_dir hit a sigstore tlog conflict (attempt $attempt/$max_attempts) — retrying in ${backoff_seconds}s" >&2
    sleep "$backoff_seconds"
    attempt=$((attempt + 1))
    continue
  fi

  # Genuine failure — surface it as-is.
  exit "$rc"
done
