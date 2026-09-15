#!/usr/bin/env python3
"""Compute which gated CI components a change can affect.

Replaces hand-maintained per-crate path filters (which silently drift from
the real dependency graph) with two sources of truth:

  * file globs for self-contained, non-Rust inputs (dart-packages/**, ...)
  * the workspace dependency graph from `cargo metadata` for crate inputs,
    via reverse transitive reachability: a component is affected when a
    changed crate -- or any crate that transitively depends on it -- is one
    of the component's root crates.

A new crate or a new dependency edge is picked up automatically because the
graph is read live from the workspace manifests; nobody has to remember to
add a path to a filter.

Outputs (one `name=true|false` per line): infra, cbor, dart, dart_pkgs, ts,
changelog, npm, cli. Writes to $GITHUB_OUTPUT when set, else stdout.

Base semantics match dorny/paths-filter: on pull_request the merge-base with
the PR base is used; on push the previous commit is used. workflow_dispatch
results are unused (gated jobs bypass on that event) but still computed.
"""

from __future__ import annotations

import json
import os
import subprocess
from fnmatch import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES_DIR = ROOT / "crates"

# Root crates per component: the crates whose build or generated output IS
# the component. A component runs when a changed crate is in this set or in
# its reverse-transitive dependency closure. These are product-structure
# facts only; the dependency graph itself is read live from cargo metadata,
# so upstream crates (cratestack-core, cratestack-parser, ...) never need to
# be listed here -- a change to them reaches every crate that depends on
# them through the graph.
ROOTS = {
    "cbor": {
        "cratestack-cbor-napi",
        "cratestack-cbor-wasm",
        "cratestack-client-flutter",
        "cratestack-client-dart",
        "cratestack-codec-cbor",
    },
    "dart": {
        "cratestack-client-dart",
        "cratestack-client-flutter",
        "cratestack-cbor-wasm",
        "cratestack-cbor-napi",
        "cratestack-codec-cbor",
        "cratestack-cli",
    },
    "ts": {
        "cratestack-client-typescript",
        "cratestack-client-dart",
        "cratestack-client-flutter",
        "cratestack-client",
        "cratestack-cli",
        "cratestack-parser",
        "cratestack-macros",
        "cratestack-sql",
        "cratestack-cbor-napi",
        "cratestack-cbor-wasm",
        "react-vite-refine-example",
        "react-vite-swr-example",
    },
    "cli": {"cratestack-cli"},
}

# Non-Rust file globs per component, relative to the workspace root. These
# directories are not part of the Cargo graph, so they stay file-driven.
GLOBS = {
    "infra": [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "justfile",
        ".ci/**",
        ".github/**",
    ],
    "cbor": [
        "dart-packages/cratestack_cbor/**",
        "examples/**",
    ],
    "dart": [
        "dart-packages/**",
        "examples/**",
    ],
    "dart_pkgs": [
        "dart-packages/cratestack_annotations/**",
        "dart-packages/cratestack_builder/**",
    ],
    "ts": [
        "packages/**",
        "examples/**",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        "package.json",
        "turbo.json",
        "biome.json",
    ],
    "changelog": [
        "**/CHANGELOG.md",
        "CHANGELOG.md",
        ".ci/changelog*",
    ],
    "npm": [
        "packages/cratestack-cli-npm/**",
        ".ci/npm-publish*",
    ],
}

OUTPUT_ORDER = ["infra", "cbor", "dart", "dart_pkgs", "ts", "changelog", "npm", "cli"]


def run(cmd, **kwargs):
    return subprocess.run(cmd, capture_output=True, text=True, **kwargs)


def git(args):
    return run(["git", *args])


def changed_files(base):
    diff = git(["diff", "--name-only", f"{base}...HEAD", "--"])
    if diff.returncode != 0:
        raise SystemExit(f"git diff {base}...HEAD failed: {diff.stderr.strip()}")
    return [line for line in diff.stdout.splitlines() if line]


def load_metadata():
    out = run(["cargo", "metadata", "--format-version", "1", "--locked"])
    if out.returncode != 0:
        raise SystemExit(f"cargo metadata failed: {out.stderr.strip()}")
    return json.loads(out.stdout)


def reverse_closure(changed_names, meta):
    """All workspace crates reachable from the changed crates via reverse
    (who-depends-on-whom) edges, including the changed crates themselves."""
    nodes = {n["id"]: set(n["dependencies"]) for n in meta["resolve"]["nodes"]}
    name_by_id = {p["id"]: p["name"] for p in meta["packages"]}
    workspace_ids = {p["id"] for p in meta["packages"] if p["source"] is None}
    reverse = {wid: set() for wid in workspace_ids}
    for nid, deps in nodes.items():
        if nid not in workspace_ids:
            continue
        for dep in deps:
            if dep in workspace_ids:
                reverse[dep].add(nid)
    id_by_name = {p["name"]: p["id"] for p in meta["packages"] if p["source"] is None}

    affected = set()
    stack = []
    for name in changed_names:
        if name in id_by_name:
            affected.add(name)
            stack.append(name)
    while stack:
        name = stack.pop()
        nid = id_by_name[name]
        for up in reverse[nid]:
            up_name = name_by_id[up]
            if up_name not in affected:
                affected.add(up_name)
                stack.append(up_name)
    return affected


def map_changed_crates(files, meta):
    """Map changed files under crates/ to workspace package names.

    Returns (names, unknown) where unknown means a crates/** path that no
    workspace package owns (e.g. the intentionally excluded
    cratestack-studio-ui). Unknown paths default to "affects everything" in
    compute() so they can only over-run, never silently skip.
    """
    pkg_by_dir = {}
    for p in meta["packages"]:
        if p["source"] is None:
            pkg_by_dir[str(Path(p["manifest_path"]).parent)] = p["name"]
    dirs = sorted(pkg_by_dir, key=len, reverse=True)

    names = set()
    unknown = False
    crates_root = str(CRATES_DIR.resolve())
    for f in files:
        path = str((ROOT / f).resolve())
        if not path.startswith(crates_root + os.sep):
            continue
        for d in dirs:
            if path.startswith(d + os.sep):
                names.add(pkg_by_dir[d])
                break
        else:
            unknown = True
    return names, unknown


def matches_any(relpath, patterns):
    return any(fnmatch(relpath, p) for p in patterns)


def compute(files, meta):
    """Pure function: changed file list + cargo metadata -> component flags."""
    flags = {comp: any(matches_any(f, patterns) for f in files)
             for comp, patterns in GLOBS.items()}

    changed_names, unknown = map_changed_crates(files, meta)
    affected = reverse_closure(changed_names, meta) if changed_names else set()

    for comp, roots in ROOTS.items():
        if affected & roots:
            flags[comp] = True

    if unknown:
        for comp in ROOTS:
            flags[comp] = True
    return flags


def main():
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    pr_base = os.environ.get("PR_BASE_SHA", "")
    base = pr_base if (event == "pull_request" and pr_base) else "HEAD~1"

    files = changed_files(base)
    meta = load_metadata()
    flags = compute(files, meta)

    lines = [f"{comp}={str(bool(flags.get(comp, False))).lower()}"
             for comp in OUTPUT_ORDER]
    out_path = os.environ.get("GITHUB_OUTPUT")
    if out_path:
        with open(out_path, "a") as fh:
            fh.write("\n".join(lines) + "\n")
    else:
        print("\n".join(lines))


if __name__ == "__main__":
    main()
