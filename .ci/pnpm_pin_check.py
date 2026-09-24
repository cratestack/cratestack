"""pnpm version pin agreement check (cratestack#1050). See pnpm-pin-check.sh.

Reads NUL-separated tracked `package.json` paths on stdin (from
`git ls-files -z`) and enforces, against the root `package.json`'s
`packageManager`:

1. every `packageManager` pin equals the root's;
2. every `devEngines.packageManager` version, when present, equals the root's
   version (pnpm honours that field too, so it is a second place to drift);
3. every manifest-only workspace root declares a pin. That is a
   `package.json` with a `pnpm-workspace.yaml` beside it and no dependencies
   of its own: the shape added so Dependabot can maintain an example's
   lockfile (#1049, #1050), where the pin is the only thing it carries. A
   deleted pin would otherwise pass rule 1 by having nothing to compare.

The root must declare a pin; with nothing to compare against, the check fails
as a setup error rather than passing vacuously.
"""

import json
import os
import sys

DEP_KEYS = ("dependencies", "devDependencies", "peerDependencies", "optionalDependencies")


def load(path):
    # utf-8-sig: a BOM is legal in JSON files and must not hide a pin.
    with open(path, encoding="utf-8-sig") as f:
        return json.load(f)


def dev_engines_version(manifest):
    pm = (manifest.get("devEngines") or {}).get("packageManager")
    if isinstance(pm, dict) and pm.get("name") == "pnpm":
        return pm.get("version")
    return None


def main():
    paths = [p for p in sys.stdin.buffer.read().decode().split("\0") if p]
    if "package.json" not in paths:
        sys.exit("pnpm-pin-check: setup error: no tracked root package.json")

    root = load("package.json").get("packageManager")
    if not root or not root.startswith("pnpm@"):
        sys.exit(f"pnpm-pin-check: setup error: root package.json pins {root!r}, expected pnpm@<version>")
    root_version = root.removeprefix("pnpm@").split("+", 1)[0]

    errors, pinned = [], []
    for path in sorted(paths):
        if path == "package.json":
            continue
        manifest = load(path)
        pin = manifest.get("packageManager")
        if pin is not None:
            pinned.append(path)
            if pin != root:
                errors.append((path, f"packageManager is {pin!r}, but the root package.json pins {root!r}"))
        dev = dev_engines_version(manifest)
        if dev is not None and dev != root_version:
            errors.append((path, f"devEngines.packageManager version is {dev!r}, but the root pins {root_version!r}"))
        workspace_root = os.path.exists(os.path.join(os.path.dirname(path), "pnpm-workspace.yaml"))
        manifest_only = not any(manifest.get(k) for k in DEP_KEYS)
        if workspace_root and manifest_only and pin is None:
            errors.append((path, f"manifest-only pnpm workspace root declares no packageManager; pin {root!r}"))

    for path, message in errors:
        print(f"::error file={path}::{message}")
    if errors:
        sys.exit(f"pnpm-pin-check: {len(errors)} problem(s) against the root pin ({root})")

    listed = ", ".join(pinned) or "(none)"
    print(f"pnpm-pin-check: ok, root {root} and {len(pinned)} other pin(s) agree: {listed}")


if __name__ == "__main__":
    main()
