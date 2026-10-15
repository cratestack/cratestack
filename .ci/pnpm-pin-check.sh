#!/usr/bin/env bash
# pnpm version pin agreement check (cratestack#1050, #1060).
#
# The pnpm version is pinned with `packageManager` in every tracked
# `package.json` that sits beside a tracked `pnpm-lock.yaml` (the repo root,
# and every example lockfile root), and in the manifest-only roots of the
# example workspaces that exist so Dependabot can maintain their lockfiles
# (`examples/react-vite-swr`, #1049; `examples/react-nextjs-daisyui`, #1050).
# Dependabot resolves each example with that example's pin, and pnpm itself
# switches to it too: run inside the example, even CI's root-pinned pnpm
# hands over to the example's version (measured: an example pin of 11.23.0
# makes `pnpm --version` there print 11.23.0). So a drifted pin silently runs
# that example on a different pnpm from the rest of CI, and every comment
# written against the root's version (e.g. the pnpm/pnpm#14987 note in
# ci.yml) stops being true there. Nothing else compares them.
#
# Rules and edge cases live in `.ci/pnpm_pin_check.py`'s docstring.
#
# Every tracked pnpm lockfile root must declare a pin (#1060): the roots are
# derived from `git ls-files`, so a new lockfile is covered automatically.
#
# Run locally via `just verify-pnpm-pins`.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/.."

if ! command -v python3 > /dev/null; then
  echo "python3 not found on PATH" >&2
  exit 1
fi

git ls-files -z -- '*package.json' '*pnpm-lock.yaml' | python3 "$script_dir/pnpm_pin_check.py"
