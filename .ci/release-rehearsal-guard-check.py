#!/usr/bin/env python3
"""Prove a release rehearsal cannot write to any registry (cratestack#652).

`release-cli.yml` gained a `rehearsal` input that runs every build and
every content gate while publishing nothing. Its acceptance criterion is
deliberately phrased as a *structural* one:

    "The rehearsal cannot publish under any input combination — verified
     by inspection of every publish step's guard, not by trusting a flag."

That is this script. It is the inspection, run automatically, so the
property is checked on every PR rather than asserted once in a review.

WHY A SCRIPT AND NOT A REVIEW NOTE. This workflow cannot be exercised on a
PR — its first execution against any change *is* a production release,
which is the entire reason #652 exists. So the only defence against
someone later adding a publish step without a guard is a machine that
reads the file. A human "I checked" does not survive the next
contributor.

THE RULE. Every step that performs an irreversible write must be reachable
only when `github.event_name == 'push'` — i.e. a real tag push. Two ways a
step can satisfy that:

  1. The step itself carries `if: github.event_name == 'push'`.
  2. The step delegates to a wrapper that is rehearsal-aware, and wires the
     rehearsal signal through (currently only `npm-publish.sh` via
     `NPM_PUBLISH_REHEARSAL`). This is the "share steps, don't copy them"
     shape #652's Risk 1 asks for, so it must be *allowed* — but only when
     the signal is actually wired, which is what makes it safe.

A job-level `if:` is NOT accepted as a guard. The publish jobs deliberately
run during a rehearsal (that is how the gates get exercised), so their
job-level condition is true in rehearsal mode. Only the step matters.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover - CI installs it
    print("PyYAML is required: pip install pyyaml", file=sys.stderr)
    raise SystemExit(2)

WORKFLOW = Path(__file__).resolve().parent.parent / ".github/workflows/release-cli.yml"

# Commands that write somewhere the project cannot take back. Deliberately
# broad: a false positive costs one `if:` line, a false negative costs a
# version number.
IRREVERSIBLE = re.compile(
    r"""
      \bnpm\s+publish\b
    | \bnpm\s+dist-tag\b
    | \bpnpm\s+publish\b
    | \bpub\s+publish\b
    | \brelease-publish\s+real\b
    | npm-publish\.sh
    | softprops/action-gh-release
    | \bgh\s+release\s+(create|upload|edit)\b
    | \bcargo\s+publish\b(?!.*--dry-run)
    """,
    re.VERBOSE | re.IGNORECASE,
)

PUSH_GUARD = "github.event_name == 'push'"
# The one rehearsal-aware wrapper. Extending this set is a deliberate act:
# a new entry means "this command knows how to not-publish", and the
# wiring check below is what stops it becoming a rubber stamp.
REHEARSAL_AWARE = {"npm-publish.sh": "NPM_PUBLISH_REHEARSAL"}


def executable_lines(step: dict) -> str:
    """The lines of a step that actually RUN, with prose stripped out.

    Scanning the raw `run:` block produced two false positives on the first
    version of this script, both found by running it rather than by
    reasoning about it:

      - `preflight`'s OIDC check contains the words "npm publish jobs"
        inside an `::error::` message.
      - the pub.dev archive gate runs `dart pub publish --dry-run`, which
        is the gate itself, not a write.

    Both are checker bugs, not workflow bugs, and papering over them by
    special-casing those two steps would have been the wrong fix — the
    next prose mention of `npm publish` would fail the build again. So:
    drop shell comments, drop lines that only print text, and drop
    `--dry-run` invocations, then match what is left.
    """
    kept = []
    for line in (step.get("run") or "").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if "--dry-run" in stripped:
            continue
        # `echo "... npm publish ..."` and `printf` are prose, not writes.
        if re.match(r'^(echo|printf)\b', stripped):
            continue
        kept.append(stripped)
    kept.append(step.get("uses") or "")
    return "\n".join(kept)


def main() -> int:
    workflow = yaml.safe_load(WORKFLOW.read_text())
    problems: list[str] = []
    checked = 0

    for job_name, job in (workflow.get("jobs") or {}).items():
        for index, step in enumerate(job.get("steps") or []):
            blob = executable_lines(step)
            if not IRREVERSIBLE.search(blob):
                continue
            checked += 1
            label = step.get("name") or step.get("uses") or f"step #{index}"
            step_if = str(step.get("if", ""))

            if PUSH_GUARD in step_if:
                continue

            wrapper = next((w for w in REHEARSAL_AWARE if w in blob), None)
            if wrapper is not None:
                required_env = REHEARSAL_AWARE[wrapper]
                env = step.get("env") or {}
                if required_env in env and "rehearsal" in str(env[required_env]):
                    continue
                problems.append(
                    f"{job_name} -> {label}: delegates to {wrapper}, which is "
                    f"rehearsal-aware, but does not wire {required_env} to "
                    f"prepare's `rehearsal` output. In a rehearsal this would "
                    f"PUBLISH."
                )
                continue

            problems.append(
                f"{job_name} -> {label}: performs an irreversible write with no "
                f"`if: {PUSH_GUARD}` guard (found: {step_if or 'no if:'}). A "
                f"rehearsal would reach it."
            )

    if checked == 0:
        # A pattern that matches nothing is the silent failure mode for a
        # check like this — it would report success forever while proving
        # nothing. Treat it as a failure, not a pass.
        print(
            "FAIL: found no irreversible publish steps at all. The IRREVERSIBLE "
            "pattern has drifted from the workflow — this check is no longer "
            "checking anything.",
            file=sys.stderr,
        )
        return 1

    if problems:
        print(
            f"FAIL: {len(problems)} publish step(s) reachable during a rehearsal:\n",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "\nEvery irreversible write must carry `if: github.event_name == "
            "'push'`, or delegate to a rehearsal-aware wrapper with its signal "
            "wired through. See cratestack#652.",
            file=sys.stderr,
        )
        return 1

    print(f"OK: all {checked} irreversible publish step(s) are guarded against a rehearsal.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
