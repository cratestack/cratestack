#!/usr/bin/env python3
"""Layer-direction check core logic (ADR 0014).

Invoked by `.ci/layer-direction-check.sh`, which pipes `cargo metadata
--no-deps --format-version=1` into this script's stdin. Not meant to be run
standalone in normal use (though it works fine with a pre-saved metadata
file: `cargo metadata --no-deps --format-version=1 > m.json && python3
.ci/layer_direction_check.py --layers docs/adr/layers.toml --allowlist
.ci/layer-direction-allowlist.toml < m.json`).

Pure stdlib: `tomllib` (Python 3.11+) plus `json`/`argparse`/`sys`. No
third-party dependency to install in CI.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from dataclasses import dataclass


@dataclass(frozen=True)
class Edge:
    src: str
    dst: str


def load_manifest(path: str) -> tuple[dict[str, int], set[str], set[str]]:
    with open(path, "rb") as f:
        data = tomllib.load(f)
    layers: dict[str, int] = dict(data.get("layers", {}))
    tools: set[str] = set(data.get("tools", {}).keys())
    vitrine: set[str] = set(data.get("vitrine", {}).keys())
    return layers, tools, vitrine


def load_allowlist(path: str) -> dict[Edge, dict]:
    with open(path, "rb") as f:
        data = tomllib.load(f)
    out: dict[Edge, dict] = {}
    for entry in data.get("allow", []):
        for required in ("from", "to", "issue", "added", "reason"):
            if required not in entry:
                print(
                    f"::error::{path}: allowlist entry missing required field '{required}': {entry}",
                    file=sys.stderr,
                )
                sys.exit(1)
        edge = Edge(entry["from"], entry["to"])
        if edge in out:
            print(
                f"::error::{path}: duplicate allowlist entry for {edge.src} -> {edge.dst}",
                file=sys.stderr,
            )
            sys.exit(1)
        out[edge] = entry
    return out


def workspace_crates(metadata: dict) -> list[dict]:
    # Scope: only packages whose manifest lives under `crates/`. `examples/*`
    # and app-shaped workspace members are consumers of the layered
    # architecture, not part of it — neither layering.md nor ADR 0011
    # assigns them a layer.
    return [p for p in metadata["packages"] if "/crates/" in p["manifest_path"]]


def normal_edges(pkgs: list[dict]) -> list[Edge]:
    edges: list[Edge] = []
    names = {p["name"] for p in pkgs}
    for p in pkgs:
        src = p["name"]
        for dep in p["dependencies"]:
            dst = dep["name"]
            if dst not in names:
                continue  # not a cratestack/crates-scoped crate (external dep)
            if dst == src:
                continue
            # cargo metadata: kind is null for normal deps, "dev" for
            # dev-dependencies, "build" for build-dependencies. Both dev and
            # build are exempt (see script header for the build rationale).
            # This intentionally does NOT branch on `target` — a
            # target-gated normal dependency (kind still null, target set)
            # must be checked exactly like an unconditional one.
            if dep.get("kind") is not None:
                continue
            edges.append(Edge(src, dst))
    return edges


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--layers", required=True)
    parser.add_argument("--allowlist", required=True)
    args = parser.parse_args()

    metadata = json.load(sys.stdin)
    layers, tools, vitrine = load_manifest(args.layers)
    allowlist = load_allowlist(args.allowlist)

    pkgs = workspace_crates(metadata)
    crate_names = {p["name"] for p in pkgs}
    assigned = set(layers) | tools | vitrine

    failed = False

    # --- Manifest completeness -------------------------------------------
    unassigned = sorted(crate_names - assigned)
    if unassigned:
        failed = True
        for name in unassigned:
            print(
                f"::error::{args.layers}: crate '{name}' exists under crates/ "
                "but has no entry in [layers]/[tools]/[vitrine] — a workspace "
                "crate must be assigned a layer in the PR that adds it "
                "(ADR 0014).",
                file=sys.stderr,
            )

    stale_manifest = sorted(assigned - crate_names)
    if stale_manifest:
        for name in stale_manifest:
            print(
                f"::warning::{args.layers}: '{name}' is assigned a layer but no "
                "longer exists under crates/ — remove the stale entry.",
                file=sys.stderr,
            )

    # --- Direction check ---------------------------------------------------
    edges = normal_edges(pkgs)
    used_allowlist: set[Edge] = set()
    real_violations: list[tuple[Edge, str]] = []
    allowlisted_violations: list[tuple[Edge, str]] = []

    for edge in edges:
        src, dst = edge.src, edge.dst
        if src in unassigned or dst in unassigned:
            continue  # already reported above; don't double-count

        # Tools may depend on anything.
        if src in tools or src in vitrine:
            continue

        # Nothing outside the tool/vitrine set may depend on a tool or the
        # vitrine crate (ADR 0011 decision 2 / ADR 0014 decision 3).
        if dst in tools or dst in vitrine:
            role = "tool" if dst in tools else "vitrine"
            reason = f"{dst} is a {role} crate; only tools may depend on a tool"
            if edge in allowlist:
                used_allowlist.add(edge)
                allowlisted_violations.append((edge, reason))
            else:
                real_violations.append((edge, reason))
            continue

        src_layer = layers[src]
        dst_layer = layers[dst]
        if dst_layer > src_layer:
            reason = f"L{src_layer} -> L{dst_layer}"
            if edge in allowlist:
                used_allowlist.add(edge)
                allowlisted_violations.append((edge, reason))
            else:
                real_violations.append((edge, reason))

    if real_violations:
        failed = True
        print(
            "::error::layer-direction violations (dep.layer > self.layer, "
            "not allowlisted):",
            file=sys.stderr,
        )
        for edge, reason in real_violations:
            print(f"  {edge.src} -> {edge.dst}  ({reason})", file=sys.stderr)

    if allowlisted_violations:
        print("layer-direction violations under allowlist:")
        for edge, reason in allowlisted_violations:
            entry = allowlist[edge]
            print(
                f"  ALLOWLISTED: {edge.src} -> {edge.dst}  ({reason})  "
                f"[{entry['issue']}, added {entry['added']}]"
            )

    # --- Stale allowlist entries --------------------------------------------
    stale_entries = [e for e in allowlist if e not in used_allowlist]
    if stale_entries:
        failed = True
        for edge in sorted(stale_entries, key=lambda e: (e.src, e.dst)):
            entry = allowlist[edge]
            print(
                f"::error::{args.allowlist}: unused allowlist entry "
                f"{edge.src} -> {edge.dst} ({entry['issue']}) does not match "
                "any actual violation — either the edge was fixed (remove "
                "the entry) or the edge no longer exists (remove the "
                "entry). An allowlist entry that matches nothing is not "
                "evidence of anything.",
                file=sys.stderr,
            )

    if failed:
        print("layer-direction check: FAIL", file=sys.stderr)
        return 1

    print(
        f"layer-direction check: PASS "
        f"({len(pkgs)} crates, {len(edges)} normal cratestack-* edges checked, "
        f"{len(allowlisted_violations)} allowlisted)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
