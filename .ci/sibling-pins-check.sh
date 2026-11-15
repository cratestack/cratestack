#!/usr/bin/env bash
# Exact sibling-pin check (cratestack#1390).
#
# The public `cratestack-*` crates are versioned together, so each one must
# require its siblings with an exact `=x.y.z`, not a caret. With carets a
# consumer that pins `cratestack-client = "=0.20.0"` can still resolve
# `cratestack-core` 0.20.1 beside 0.20.0 of everything else, and the macros
# then generate code against an API they were not built with. Exact pins make
# one consumer build resolve exactly one CrateStack version, the way `serde`
# pins `serde_derive`.
#
# Manifest-only: reads TOML, compiles nothing, needs no cargo. Run locally via
# `just check-sibling-pins`. CI runs it as the `sibling-pins` job.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

if ! command -v python3 >/dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

python3 "$(dirname "${BASH_SOURCE[0]}")/sibling_pins_check.py" "$@"
