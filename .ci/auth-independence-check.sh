#!/usr/bin/env bash
# Auth-library independence check (ADR 0021 §7, cratestack#1243).
#
# CrateStack does not depend on any authentication library: the in-house
# auth and an external one (Authkestra's crates, for example) plug in through
# the public `AuthProvider` seam alone, and only examples and applications
# depend on an external library. This check fails if a package whose name
# starts with a guarded prefix appears in the workspace's resolved graph.
#
# The root `Cargo.lock` is the whole resolved graph of every workspace member
# under every feature (Cargo resolves the lockfile with all features), so a
# guarded package anywhere in it, direct or transitive, is a violation. An
# example that wires an external library lives outside the workspace, with
# its own lockfile (`examples/client-only-verification` is the precedent), so
# it is never in this graph.
#
# Usage: .ci/auth-independence-check.sh [prefix ...]   (default: authkestra)
set -euo pipefail

cd "$(dirname "$0")/.."
prefixes=("$@")
if [ ${#prefixes[@]} -eq 0 ]; then
    prefixes=(authkestra)
fi

status=0
for prefix in "${prefixes[@]}"; do
    found=$(grep -E "^name = \"${prefix}[^\"]*\"$" Cargo.lock || true)
    if [ -n "$found" ]; then
        echo "::error::Cargo.lock: the workspace graph contains '${prefix}*' packages:" >&2
        echo "$found" >&2
        echo "A framework crate must not depend on an authentication library (ADR 0021 §7)." >&2
        echo "Find the edge with: cargo tree --workspace -i <package>" >&2
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "auth independence check: PASS (no ${prefixes[*]} package in Cargo.lock)"
fi
exit "$status"
