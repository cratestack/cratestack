#!/usr/bin/env python3
"""File-length ceiling check — see `.ci/file-length-check.sh` for rationale.

Reads an allowlist of grandfathered files and fails when either:

  * a file outside the allowlist exceeds the ceiling (a NEW violation), or
  * an allowlist entry names a file that no longer exceeds the ceiling, or
    no longer exists (a STALE entry).

The stale-entry rule is deliberate and mirrors
`.ci/layer-direction-allowlist.toml`: an allowlist entry nobody removed is
exactly as rotten as an `#[allow(dead_code)]` nobody removed. Removing the
entry belongs in the same PR that shrinks the file.
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

REQUIRED_ENTRY_KEYS = ("path", "lines", "issue", "added", "reason")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allowlist", required=True, type=Path)
    parser.add_argument("--limit", required=True, type=int)
    parser.add_argument(
        "--root",
        action="append",
        required=True,
        dest="roots",
        help="Glob (relative to the project root) selecting files to check. Repeatable.",
    )
    return parser.parse_args()


def load_allowlist(path: Path) -> dict[str, dict]:
    if not path.is_file():
        raise SystemExit(f"allowlist not found: {path}")

    document = tomllib.loads(path.read_text(encoding="utf-8"))
    entries: dict[str, dict] = {}

    for entry in document.get("file", []):
        missing = [key for key in REQUIRED_ENTRY_KEYS if key not in entry]
        if missing:
            raise SystemExit(
                f"{path}: allowlist entry {entry!r} is missing required key(s): "
                f"{', '.join(missing)}"
            )
        if entry["path"] in entries:
            raise SystemExit(f"{path}: duplicate allowlist entry for {entry['path']}")
        entries[entry["path"]] = entry

    return entries


def collect_files(roots: list[str]) -> list[Path]:
    found: set[Path] = set()
    for pattern in roots:
        found.update(match for match in Path().glob(pattern) if match.is_file())
    return sorted(found)


def line_count(path: Path) -> int:
    with path.open("rb") as handle:
        return sum(1 for _ in handle)


def main() -> int:
    args = parse_args()
    allowlist = load_allowlist(args.allowlist)

    new_violations: list[tuple[str, int]] = []
    stale_entries: list[tuple[str, str]] = []
    seen: set[str] = set()

    for file_path in collect_files(args.roots):
        key = file_path.as_posix()
        seen.add(key)
        lines = line_count(file_path)

        if key in allowlist:
            if lines <= args.limit:
                stale_entries.append(
                    (key, f"now {lines} lines, at or under the {args.limit}-line ceiling")
                )
        elif lines > args.limit:
            new_violations.append((key, lines))

    for key in allowlist:
        if key not in seen:
            stale_entries.append((key, "file no longer exists or is outside the checked roots"))

    if new_violations:
        print(
            f"error: {len(new_violations)} file(s) exceed the {args.limit}-line ceiling "
            "and are not in the allowlist:",
            file=sys.stderr,
        )
        for key, lines in sorted(new_violations, key=lambda item: -item[1]):
            print(f"  {lines:>5}  {key}", file=sys.stderr)
        print(
            "\nSplit the file by concern (see CLAUDE.md, '200-LoC file ceiling'). "
            "Move inline `#[cfg(test)] mod tests { ... }` bodies into a sibling "
            "`tests.rs` — that alone brings most files under the ceiling.",
            file=sys.stderr,
        )

    if stale_entries:
        print(
            f"\nerror: {len(stale_entries)} stale allowlist entr(y/ies) in {args.allowlist}:",
            file=sys.stderr,
        )
        for key, why in sorted(stale_entries):
            print(f"  {key} — {why}", file=sys.stderr)
        print(
            "\nRemove the entry in the same PR that fixed the file. An allowlist "
            "entry nobody removes is a second, silent way to disable this check.",
            file=sys.stderr,
        )

    if new_violations or stale_entries:
        return 1

    checked = len(seen)
    print(
        f"file-length check passed: {checked} file(s) at or under {args.limit} lines "
        f"({len(allowlist)} grandfathered)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
