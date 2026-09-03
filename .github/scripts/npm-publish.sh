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
# 3. (v0.11.1, run 33808493207, 2026-09-03 — during npm's "Intermittent
#    Failures Impacting npm Publish" incident) Transient registry auth
#    failure: `E401 ... Failed to generate Web Auth URLs due to error:
#    BadRequestError: token is invalid` on an OIDC publish whose provenance
#    had ALREADY been accepted by sigstore. Retrying the whole command
#    cleared it on attempt 2 for four Linux legs, so it is retried here.
#
# 4. Version already STAGED upstream (stop retrying, treat as accepted).
#    `E409 ... Cannot publish over previously staged version "X"` means an
#    earlier attempt — possibly one that returned 401 to us — was accepted
#    by the registry and is being processed. Retrying can only ever get
#    the same 409, and the version may appear minutes later. So: exit 0
#    with a loud `::warning::`, and leave it to the caller's post-publish
#    registry verification (publish-npm-cbor-node's "Verify every
#    subpackage is visible on the registry" step) to decide whether the
#    release is actually complete. This is the same posture as (2): npm
#    already holds the tarball; there is nothing this script can add.
#
# HOW FAILURES ARE CLASSIFIED — read this before adding a pattern. Only
# `npm error` lines are consulted. The first version of this script
# grepped the WHOLE output for `transparency log`, which also matches the
# informational notice npm prints on EVERY publish ("Provenance statement
# published to transparency log: ...") — so every failure, of any kind,
# was labelled "sigstore tlog conflict" and retried four times. On
# v0.11.1 that turned a 401 into a three-minute retry loop that ended in a
# 409 "previously staged" the script then reported as a tlog failure.
# `.ci/npm-publish-tests.sh` pins the classification with the real 0.11.1
# outputs; its "permanent error with the notice line present" case fails
# against that first version.
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

# REHEARSAL MODE (cratestack#652). Set by the release workflow when it is
# rehearsing rather than releasing. Runs the full pack + validation path
# and writes to no registry.
#
# It lives HERE, in the one wrapper all seven npm publishes already share,
# rather than as a second `if:`-guarded step per job. That is #652's Risk 1
# ("a rehearsal that becomes a second, diverging copy of the pipeline")
# taken seriously: there is exactly one code path, and the rehearsal
# differs from the release by two flags rather than by being a separate
# transcription that can drift.
#
# `--provenance` is DROPPED, not passed through. It mints a sigstore
# attestation tied to a real publish; with `--dry-run` there is nothing to
# attest and npm errors out. Dropping it is therefore not a weakening of
# the rehearsal — it is the only way the dry run reaches the packing and
# validation this exists to exercise.
#
# ALREADY-PUBLISHED IS A REHEARSAL PASS, and this is measured rather than
# reasoned. A first draft of this block asserted that a dry run "cannot
# collide with a published version" and skipped the check below. That is
# false: `npm publish --dry-run` still talks to the registry, and running
# it against this repo's own `packages/cratestack-refine` at an
# already-released version produced
#
#     npm error You cannot publish over the previously published versions: 0.8.15.
#
# after printing the full tarball manifest. The pack and validation — the
# entire point of the rehearsal — had already succeeded at that point. So
# the same "already published" text the real path treats as success is
# treated as success here, for the same reason. Without this, rehearsing
# any already-released version reports a failure that is not one, which is
# the false-RED mirror of the false-green this ticket exists to kill.
#
# The sigstore tlog retry below is genuinely unreachable here, because
# `--provenance` is dropped and no attestation is written.
if [ "${NPM_PUBLISH_REHEARSAL:-0}" = "1" ]; then
  dry_args=()
  for arg in "$@"; do
    [ "$arg" = "--provenance" ] && continue
    dry_args+=("$arg")
  done
  echo "npm-publish: REHEARSAL — 'npm publish --dry-run' in $pkg_dir, no registry write" >&2
  out=$(cd "$pkg_dir" && npm publish "${dry_args[@]}" --dry-run 2>&1)
  rc=$?
  printf '%s\n' "$out"
  if [ "$rc" -eq 0 ]; then
    exit 0
  fi
  if printf '%s' "$out" | grep -qi 'cannot publish over the previously published version'; then
    echo "npm-publish: REHEARSAL — $pkg_dir is already published at this version; the tarball packed and validated, which is what the rehearsal checks" >&2
    exit 0
  fi
  echo "::error::npm publish --dry-run failed for $pkg_dir for a reason other than the version already existing. The real publish would fail too." >&2
  exit "$rc"
fi

# Overridable so the retry path can be exercised without a minute of
# real sleeping; CI uses the defaults.
max_attempts=${NPM_PUBLISH_MAX_ATTEMPTS:-4}
backoff_seconds=${NPM_PUBLISH_BACKOFF_SECONDS:-20}
attempt=1
while true; do
  out=$(cd "$pkg_dir" && npm publish "$@" 2>&1)
  rc=$?
  printf '%s\n' "$out"
  if [ "$rc" -eq 0 ]; then
    exit 0
  fi
  # Classify on `npm error` lines ONLY — see the header. The provenance
  # notice ("... published to transparency log: ...") is `npm notice`,
  # not `npm error`, and must never reach these patterns.
  errs=$(printf '%s\n' "$out" | grep -i '^npm error' || true)
  if printf '%s' "$errs" | grep -qi 'cannot publish over the previously published version'; then
    echo "npm-publish: $pkg_dir is already published at this version — treating as success" >&2
    exit 0
  fi
  if printf '%s' "$errs" | grep -qi 'cannot publish over previously staged version'; then
    echo "::warning::npm-publish: $pkg_dir is already STAGED upstream at this version (E409 'previously staged'): an earlier attempt was accepted and npm is still processing it. Not retrying — a retry can only repeat this 409. The post-publish registry verification step decides whether it became visible." >&2
    exit 0
  fi
  transient=""
  if printf '%s' "$errs" | grep -qi 'TLOG_CREATE_ENTRY_ERROR\|transparency log'; then
    transient="sigstore tlog conflict"
  elif printf '%s' "$errs" | grep -qi 'Failed to generate Web Auth URLs'; then
    transient="transient registry auth failure (E401 'Failed to generate Web Auth URLs')"
  elif printf '%s' "$errs" | grep -qiE 'E(502|503|504)\b|ECONNRESET|ETIMEDOUT|EAI_AGAIN|socket hang up'; then
    transient="registry/network error"
  fi
  if [ -z "$transient" ]; then
    echo "npm-publish: $pkg_dir failed with a non-transient error (exit $rc) — not retrying" >&2
    exit "$rc"
  fi
  if [ "$attempt" -ge "$max_attempts" ]; then
    echo "npm-publish: $pkg_dir still failing on a $transient after $attempt attempts — giving up" >&2
    exit "$rc"
  fi
  echo "npm-publish: $pkg_dir hit a $transient (attempt $attempt/$max_attempts) — retrying in ${backoff_seconds}s" >&2
  sleep "$backoff_seconds"
  attempt=$((attempt + 1))
done
