#!/usr/bin/env bash
# napi target drift check (cratestack#850).
#
# `@cratestack/cbor-node`'s platform list is duplicated across two files that
# must agree exactly:
#
#   - `packages/cratestack-cbor-node/package.json` -> `napi.targets`, which is
#     what `@napi-rs/cli` scaffolds, validates and publishes against;
#   - `.github/workflows/release-cli.yml` -> the `build-cbor-node` job's
#     `strategy.matrix.include[].target`, which is what actually builds a
#     binary for each of them.
#
# WHY A SCRIPT AND NOT A REVIEW NOTE — the same reason
# `.ci/release-rehearsal-guard-check.py` exists, and it is not hypothetical:
# `release-cli.yml` cannot be exercised on an ordinary PR, because its first
# execution against any change is a production release. So a mismatch produces
# no signal at all until a `v*` tag is already pushed, and then:
#
#   - A target in `napi.targets` with no matrix leg: `napi artifacts` aborts
#     with `Missing artifacts for configured targets: <triple>` and
#     `napi prepublish` with `Release package directory does not exist`, so
#     `publish-npm-cbor-node` fails. Both validate ALL configured targets
#     before touching any of them, so nothing partial ships either.
#   - A matrix leg with no `napi.targets` entry: the leg burns runner minutes
#     building a binary `napi artifacts` then ignores, and that platform
#     silently never reaches npm — which is the exact shape of the defect
#     cratestack#850 reports (a platform users needed, absent, CI green).
#
# Both directions are therefore hard failures, as is a list this script cannot
# find at all (renamed job, restructured matrix, moved field) — a checker that
# reads nothing and prints OK is worse than no checker.
#
# Parsing lives in `napi_targets_check.py` (JSON + YAML, no TOML, so this
# follows `layer-direction-check.sh`'s bash-entrypoint/python-parser shape
# rather than trying to do set logic across two formats in shell). PyYAML is
# the one non-stdlib dependency; it is checked for explicitly below so a
# missing module fails as a setup error rather than as a drift report.
#
# Run locally via `just verify-napi-targets`. CI runs that same recipe as the
# `napi-targets` job in `.github/workflows/ci.yml`.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Overridable (relative to the project root) so the self-checks can point this
# at a fixture without editing the tracked files.
PACKAGE_JSON="${NAPI_TARGETS_PACKAGE_JSON:-packages/cratestack-cbor-node/package.json}"
WORKFLOW="${NAPI_TARGETS_WORKFLOW:-.github/workflows/release-cli.yml}"
JOB="${NAPI_TARGETS_JOB:-build-cbor-node}"

if ! command -v python3 > /dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

if ! python3 -c "import yaml" > /dev/null 2>&1; then
  echo "PyYAML not found. Install it with: pip install pyyaml" >&2
  exit 2
fi

python3 "$(dirname "${BASH_SOURCE[0]}")/napi_targets_check.py" \
  --package-json "$PACKAGE_JSON" \
  --workflow "$WORKFLOW" \
  --job "$JOB"
