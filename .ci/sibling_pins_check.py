#!/usr/bin/env python3
"""Exact sibling-pin check core logic (cratestack#1390).

Invoked by `.ci/sibling-pins-check.sh`. Pure stdlib (`tomllib`, Python 3.11+).
An optional first argument names the repository root (the fixture tests in
`.ci/sibling-pins-tests.sh` use it); the default is this file's repository.

Fails when a `cratestack-*` (or the bare `cratestack`) requirement that carries a `version` is not an
exact `=x.y.z`, in either place a sibling can be named:
  - the root `[workspace.dependencies]` table (every entry, published or not,
    because `just bump` rewrites them all together);
  - every member named by the root `[workspace] members` (globs expanded,
    `exclude` honoured) that is published (`publish` is neither `false` nor
    `[]`), in
    `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` and the
    same tables under `[target.*]`. A renamed dependency
    (`cratestack = { package = "cratestack-api", ... }`) is judged by its
    `package`, not its key.

A `workspace = true` dependency is fine: it inherits the checked root entry.
A path-only dependency has no requirement and cannot be published, so it is
not this check's concern.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parent.parent
EXACT = re.compile(r"^=\s*\d+\.\d+\.\d+([-+][0-9A-Za-z.+-]*)?$")
DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def is_sibling(name: str) -> bool:
    return name == "cratestack" or name.startswith("cratestack-")


def sibling_problems(deps: dict, where: str, version: str) -> list[str]:
    out = []
    for key, spec in deps.items():
        if isinstance(spec, str):
            name, req = key, spec
        else:
            name, req = spec.get("package", key), spec.get("version")
        if not is_sibling(name) or req is None:
            continue
        if not EXACT.match(req.strip()):
            out.append(f"{where}: `{key}` requires {req!r}; use \"={version}\"")
    return out


def member_tables(manifest: dict):
    for t in DEP_TABLES:
        yield t, manifest.get(t, {})
    for tgt, body in manifest.get("target", {}).items():
        for t in DEP_TABLES:
            yield f"target.{tgt}.{t}", body.get(t, {})


def member_manifests(root: Path, workspace: dict) -> list[Path]:
    excluded = {(root / e).resolve() for e in workspace.get("exclude", [])}
    found = set()
    for pattern in workspace.get("members", []):
        for d in root.glob(pattern):
            if d.resolve() not in excluded and (d / "Cargo.toml").is_file():
                found.add(d / "Cargo.toml")
    return sorted(found)


def unpublished(manifest: dict) -> bool:
    return manifest.get("package", {}).get("publish", True) in (False, [])


def main(argv: list[str]) -> int:
    base = Path(argv[1]).resolve() if len(argv) > 1 else DEFAULT_ROOT
    problems = []
    root = tomllib.loads((base / "Cargo.toml").read_text())
    workspace = root.get("workspace", {})
    version = workspace.get("package", {}).get("version", "<workspace version>")
    ws = workspace.get("dependencies", {})
    problems += sibling_problems(ws, "Cargo.toml [workspace.dependencies]", version)
    checked = sum(1 for k, v in ws.items() if is_sibling(v.get("package", k) if isinstance(v, dict) else k))

    for path in member_manifests(base, workspace):
        manifest = tomllib.loads(path.read_text())
        if unpublished(manifest):
            continue
        rel = path.relative_to(base)
        for table, deps in member_tables(manifest):
            problems += sibling_problems(deps, f"{rel} [{table}]", version)

    if checked == 0:
        print("sibling-pins: found no cratestack workspace entries; the check is broken", file=sys.stderr)
        return 1
    if problems:
        print("sibling-pins: cratestack requirements must be exact (=x.y.z), cratestack#1390:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1
    print(f"sibling-pins: ok ({checked} workspace entries exact)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
