#!/usr/bin/env bash
# Authkestra stays in the example, out of the framework (cratestack#1245,
# ADR 0021 §7).
#
# `examples/auth-pluggable` is the one crate in the repository that depends
# on Authkestra. The root `Cargo.lock` cannot see it (`.ci/auth-independence-
# check.sh` covers the workspace), so this check runs `cargo tree` inside the
# example, on every CrateStack crate in its graph (each `cratestack*`
# package with a local path source), and fails if any of them reaches an
# `authkestra*` package. It also asserts that the example itself does, so
# the grep is proven able to fail, and that its `src/` names no
# `__private` path: the providers use the public seam only (ADR 0021 §7).
#
# Usage: .ci/auth-pluggable-check.sh   (from anywhere; needs `--locked` to resolve)
set -euo pipefail

cd "$(dirname "$0")/../examples/auth-pluggable"

# A proc-macro crate has no normal edges on the target, and cargo then warns
# "nothing to print": quiet, but a failing `cargo tree` still stops the
# script (`set -e`).
tree() {
    cargo tree --locked -e normal,build --prefix none -p "$1" 2>/dev/null
}

own=$(tree auth-pluggable)
if ! grep -q '^authkestra' <<<"$own"; then
    echo "::error::expected examples/auth-pluggable itself to depend on authkestra* (the check would be vacuous)" >&2
    exit 1
fi

if private=$(grep -rn '__private' src); then
    echo "::error::examples/auth-pluggable/src reaches a private CrateStack path:" >&2
    echo "$private" >&2
    exit 1
fi

packages=$(cargo metadata --locked --format-version=1 | python3 -c '
import json, sys
meta = json.load(sys.stdin)
print("\n".join(sorted({p["name"] for p in meta["packages"]
                        if p["name"].startswith("cratestack") and p["source"] is None})))
')
count=$(wc -l <<<"$packages")
if [ "$count" -lt 5 ]; then
    echo "::error::found only $count CrateStack packages in examples/auth-pluggable's graph; expected the facade and its layers" >&2
    exit 1
fi

status=0
while read -r package; do
    graph=$(tree "$package")
    if found=$(grep '^authkestra' <<<"$graph"); then
        echo "::error::$package reaches an Authkestra package in examples/auth-pluggable's graph:" >&2
        echo "$found" | sort -u >&2
        status=1
    fi
done <<<"$packages"

if [ "$status" -eq 0 ]; then
    echo "auth-pluggable check: PASS ($count CrateStack crates checked; none reaches authkestra*)"
fi
exit "$status"
