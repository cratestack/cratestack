#!/usr/bin/env bash
# Workspace-lints opt-in check (cratestack#523).
#
# `[workspace.lints.rust] unsafe_code = "forbid"` in the root `Cargo.toml`
# is inert unless every workspace member declares `[lints]\nworkspace =
# true` — Cargo silently ignores a `[workspace.lints]` table for any member
# that doesn't opt in, with no warning. #523 found that not one of the 32
# crates under `crates/` did, so the "forbidden workspace-wide" claim in
# `CLAUDE.md` had nothing enforcing it.
#
# This is the regression guard: a new crate added to `[workspace] members`
# (or to the root `Cargo.toml`'s `[workspace] exclude` list — a standalone
# example/vitrine workspace that deliberately sits outside the root graph,
# see `EXCLUDED_STANDALONE_WORKSPACES` below) without the opt-in should be
# caught here, not discovered months later the way #523 was.
#
# Manifest-only (`cargo metadata --no-deps`) — no crate is compiled.
#
# Run locally via `just verify-lints-optin`. CI runs it as the
# `lints-workspace-optin` job in `.github/workflows/ci.yml`.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

if ! command -v python3 >/dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

cargo metadata --no-deps --format-version=1 | python3 "$(dirname "${BASH_SOURCE[0]}")/lints_workspace_check.py"
