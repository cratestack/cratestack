#!/usr/bin/env bash
# pnpm version pin agreement check (cratestack#1050).
#
# The pnpm version is pinned with `packageManager` in more than one tracked
# `package.json`: the repo root, which `pnpm/action-setup` reads for every CI
# job, and the example workspace roots that exist so Dependabot can maintain
# their lockfiles (`examples/react-vite-swr`, #1049, and
# `examples/react-nextjs-daisyui`, #1050). Dependabot resolves each example
# with ITS OWN pin, while CI installs it with the ROOT pin. If the two drift,
# Dependabot writes a lockfile with one pnpm and CI checks it with another,
# and nothing else compares them.
#
# So: every tracked `package.json` that declares `packageManager` must declare
# exactly the root's value. The root must declare one; a checker with nothing
# to compare against fails as a setup error rather than passing vacuously.
#
# Deliberately NOT checked: that every lockfile root declares a pin. Eight
# example lockfile roots declare none today, and adding one changes which pnpm
# Dependabot uses there, which is a separate decision.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

git ls-files -z -- '*package.json' | python3 -c '
import json, sys

paths = [p for p in sys.stdin.buffer.read().decode().split("\0") if p]
if "package.json" not in paths:
    sys.exit("pnpm-pin-check: setup error: no tracked root package.json")

def pin(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f).get("packageManager")

root = pin("package.json")
if not root:
    sys.exit("pnpm-pin-check: setup error: root package.json declares no packageManager")

pinned = {p: pin(p) for p in paths if p != "package.json"}
pinned = {p: v for p, v in pinned.items() if v is not None}
bad = {p: v for p, v in pinned.items() if v != root}

for p, v in sorted(bad.items()):
    print(f"::error file={p}::packageManager is {v!r}, but the root package.json pins {root!r}")
if bad:
    sys.exit(f"pnpm-pin-check: {len(bad)} packageManager pin(s) disagree with the root ({root})")

listed = ", ".join(sorted(pinned)) or "(none)"
print(f"pnpm-pin-check: ok, root {root} and {len(pinned)} other pin(s) agree: {listed}")
'
