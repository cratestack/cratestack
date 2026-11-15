#!/usr/bin/env python3
"""Exact sibling-pin check core logic (cratestack#1390).

Invoked by `.ci/sibling-pins-check.sh`. Pure stdlib (`tomllib`, Python 3.11+).

Fails when a `cratestack-*` requirement that carries a `version` is not an
exact `=x.y.z`, in either place a sibling can be named:
  - the root `[workspace.dependencies]` table (every entry, published or not,
    because `just bump` rewrites them all together);
  - any `crates/*/Cargo.toml` that is published (`publish` is not false), in
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

ROOT = Path(__file__).resolve().parent.parent
EXACT = re.compile(r"^=\s*\d+\.\d+\.\d+([-+][0-9A-Za-z.+-]*)?$")
DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def sibling_problems(deps: dict, where: str) -> list[str]:
    out = []
    for key, spec in deps.items():
        if isinstance(spec, str):
            name, req = key, spec
        else:
            name, req = spec.get("package", key), spec.get("version")
        if not name.startswith("cratestack-") or req is None:
            continue
        if not EXACT.match(req.strip()):
            out.append(f"{where}: `{key}` requires {req!r}; use \"={req.lstrip('^= ')}\"")
    return out


def member_tables(manifest: dict):
    for t in DEP_TABLES:
        yield t, manifest.get(t, {})
    for tgt, body in manifest.get("target", {}).items():
        for t in DEP_TABLES:
            yield f"target.{tgt}.{t}", body.get(t, {})


def main() -> int:
    problems = []
    root = tomllib.loads((ROOT / "Cargo.toml").read_text())
    ws = root.get("workspace", {}).get("dependencies", {})
    problems += sibling_problems(ws, "Cargo.toml [workspace.dependencies]")
    checked = sum(1 for k, v in ws.items() if k.startswith("cratestack-"))

    for path in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        manifest = tomllib.loads(path.read_text())
        if manifest.get("package", {}).get("publish", True) is False:
            continue
        rel = path.relative_to(ROOT)
        for table, deps in member_tables(manifest):
            problems += sibling_problems(deps, f"{rel} [{table}]")

    if checked == 0:
        print("sibling-pins: found no cratestack-* workspace entries; the check is broken", file=sys.stderr)
        return 1
    if problems:
        print("sibling-pins: cratestack-* requirements must be exact (=x.y.z), cratestack#1390:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1
    print(f"sibling-pins: ok ({checked} workspace entries exact)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
