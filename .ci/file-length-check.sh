#!/usr/bin/env bash
# File-length ceiling check.
#
# `CLAUDE.md` declares a ~200-line ceiling per source file ("200-LoC file
# ceiling"), split by concern rather than grown — it is why `macros/` and
# `axum/` are deeply nested. Until now nothing enforced it, and the
# convention drifted the same silent way `[workspace.lints]` did before
# cratestack#523: an audit found 104 files under `crates/*/src` past the
# ceiling, the largest at 1381 lines.
#
# This is the regression guard. It is deliberately shaped like
# `.ci/layer-direction-check.sh`: a narrow, dated allowlist grandfathers
# the existing backlog so the ceiling can be enforced for NEW code today,
# and a stale allowlist entry (a file that has since been split, or that no
# longer exists) is a hard failure — an entry nobody removes is a second,
# silent way to disable the check.
#
# Scope is `crates/*/src/**/*.rs` — the framework's own source. Deliberately
# NOT covered, each for its own reason, and each a maintainer decision to
# revisit rather than an oversight:
#   * `crates/*/tests/**` — integration tests are table-driven end-to-end
#     suites (`cratestack-pg/tests/include_schema.rs` is 4015 lines); whether
#     the ceiling should apply to them is a separate call.
#   * `examples/**` — vitrine crates, several of which are single-file by
#     design to stay readable as examples.
#   * non-Rust sources — `packages/*` already keeps tests in `tests/` dirs
#     and has only 3 source files over the ceiling.
#
# Reads no Cargo metadata and compiles nothing — pure file I/O.
#
# Run locally via `just verify-file-length`.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

LIMIT="${FILE_LENGTH_LIMIT:-200}"
ALLOWLIST="${FILE_LENGTH_ALLOWLIST:-.ci/file-length-allowlist.toml}"

if ! command -v python3 >/dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

exec python3 "$(dirname "${BASH_SOURCE[0]}")/file_length_check.py" \
  --allowlist "$ALLOWLIST" \
  --limit "$LIMIT" \
  --root 'crates/*/src/**/*.rs'
