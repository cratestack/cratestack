#!/usr/bin/env python3
"""Compare `napi.targets` with the `build-cbor-node` matrix (cratestack#850).

Invoked by `.ci/napi-targets-check.sh`, which is where the rationale lives.
Paths and the job name are passed in rather than hardcoded so the self-checks
can point this at a fixture without editing the tracked files.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover - the wrapper checks for this first
    print("PyYAML is required: pip install pyyaml", file=sys.stderr)
    raise SystemExit(2)


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def package_targets(path: Path) -> list[str]:
    """`napi.targets`, or a hard failure if the shape moved.

    Deliberately not `.get(...) or []`: an empty list read from a renamed or
    restructured field would make this check pass while comparing nothing —
    the exact silent-success failure mode a guard like this exists to avoid.
    """
    try:
        data = json.loads(path.read_text())
    except FileNotFoundError:
        fail(f"could not find {path} — the package moved, and this check is reading nothing.")
    except json.JSONDecodeError as exc:
        fail(f"could not parse {path} as JSON: {exc}")

    targets = data.get("napi", {}).get("targets")
    if not isinstance(targets, list) or not targets:
        fail(
            f"could not find a non-empty `napi.targets` array in {path}. "
            f"Either the field was renamed/restructured or this check is "
            f"reading the wrong file — as written it is not proving anything."
        )
    return targets


def matrix_targets(path: Path, job: str) -> list[str]:
    """The matrix targets of `job`, or a hard failure if the job's shape moved."""
    try:
        workflow = yaml.safe_load(path.read_text())
    except FileNotFoundError:
        fail(f"could not find {path} — the workflow moved, and this check is reading nothing.")
    except yaml.YAMLError as exc:
        fail(f"could not parse {path} as YAML: {exc}")

    jobs = workflow.get("jobs") or {}
    if job not in jobs:
        fail(
            f"could not find the `{job}` job in {path} (jobs present: "
            f"{', '.join(sorted(jobs)) or 'none'}). It was renamed or removed, "
            f"so this check can no longer find the matrix it compares."
        )

    include = (jobs[job].get("strategy") or {}).get("matrix", {}).get("include")
    if not isinstance(include, list) or not include:
        fail(
            f"could not find a non-empty `strategy.matrix.include` on `{job}` "
            f"in {path}. The job's shape changed and this check is comparing "
            f"nothing."
        )

    targets = [entry.get("target") for entry in include if entry.get("target")]
    if len(targets) != len(include):
        fail(
            f"{len(include) - len(targets)} entr(y/ies) in `{job}`'s matrix have "
            f"no `target:` key. Every leg must name the triple it builds, or the "
            f"two lists cannot be compared."
        )
    return targets


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-json", required=True, type=Path)
    parser.add_argument("--workflow", required=True, type=Path)
    parser.add_argument("--job", required=True)
    args = parser.parse_args()

    pkg = package_targets(args.package_json)
    mat = matrix_targets(args.workflow, args.job)

    # Duplicates are their own bug: a repeat in `napi.targets` would scaffold
    # the same npm dir twice, and a repeated matrix leg would race two uploads
    # of one artifact name.
    for label, values in ((f"{args.package_json} napi.targets", pkg), (f"`{args.job}` matrix", mat)):
        duplicates = sorted({v for v in values if values.count(v) > 1})
        if duplicates:
            fail(f"{label} lists duplicate target(s): {', '.join(duplicates)}")

    missing_leg = sorted(set(pkg) - set(mat))
    missing_target = sorted(set(mat) - set(pkg))

    if missing_leg or missing_target:
        print(
            f"FAIL: `napi.targets` ({args.package_json}) and the `{args.job}` "
            f"matrix ({args.workflow}) disagree.\n",
            file=sys.stderr,
        )
        for target in missing_leg:
            print(
                f"  - {target}: in napi.targets, but NO matrix leg builds it. "
                f"At the next tag `napi artifacts` aborts with 'Missing "
                f"artifacts for configured targets: {target}' and `napi "
                f"prepublish` with 'Release package directory does not exist'.",
                file=sys.stderr,
            )
        for target in missing_target:
            print(
                f"  - {target}: built by a matrix leg, but MISSING from "
                f"napi.targets. The binary is built and then silently dropped; "
                f"the platform never reaches npm, with CI green throughout.",
                file=sys.stderr,
            )
        print(
            f"\nAdd the target to BOTH {args.package_json} and the `{args.job}` "
            f"matrix in {args.workflow}. See cratestack#850.",
            file=sys.stderr,
        )
        return 1

    print(f"napi-targets check passed: napi.targets and `{args.job}` both list the same {len(pkg)} target(s).")
    for target in pkg:
        print(f"  - {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
