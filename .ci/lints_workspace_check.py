#!/usr/bin/env python3
"""Workspace-lints opt-in check core logic (cratestack#523).

Invoked by `.ci/lints-workspace-check.sh`, which pipes `cargo metadata
--no-deps --format-version=1` into this script's stdin. Not meant to be run
standalone in normal use (though it works fine with a pre-saved metadata
file, same shape as `.ci/layer_direction_check.py`).

Pure stdlib: `tomllib` (Python 3.11+) plus `json`/`sys`/`pathlib`. No
third-party dependency to install in CI.

What "compliant" means for a manifest:
  - The common case: `[lints]\nworkspace = true` is present, so the
    manifest inherits the root (or, for a standalone workspace, its own)
    `[workspace.lints]` table.
  - The FFI-boundary exception (`EXEMPT_MANUAL_OVERRIDE` below): Cargo
    rejects mixing `[lints]\nworkspace = true` with any `[lints.<tool>]`
    override in the same manifest ("cannot override `workspace.lints` in
    `lints`, either remove the overrides or `lints.workspace = true` and
    manually specify the lints" — verified directly against cargo, see the
    cratestack#523 PR description). A crate that must lift `unsafe_code`
    for a real FFI boundary (napi-derive/wasm-bindgen trampolines, raw
    C-ABI exports) therefore can't use the inherited form at all; it
    manually re-declares `[lints.rust] unsafe_code = "allow"` instead. This
    script requires that exact override to be present for the three
    crates on the exemption list, so widening the exemption list (or
    silently dropping the override) both fail the check.

Excluded standalone workspaces: the root `Cargo.toml`'s `[workspace]
exclude` list holds crates that are deliberately their OWN workspace root
(not merged into the root dependency graph — see each crate's own
`[workspace]` doc comment for why), so `cargo metadata` run from the repo
root never sees them and they don't inherit the root's
`[workspace.lints]` table at all. Each such crate is required to declare
its own local `[workspace.lints.rust] unsafe_code = "forbid"` PLUS
`[lints]\nworkspace = true` to opt into it — same guarantee, declared
twice because the workspaces are genuinely disjoint. This is discovered
dynamically from the root manifest's `exclude` array (not hardcoded), so a
future excluded crate is caught by this same check without editing this
script.
"""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

# cratestack#523: napi-derive/wasm-bindgen-adjacent FFI trampolines and raw
# C-ABI exports embed `unsafe` (sometimes only visible after macro
# expansion — see the PR description's `cratestack-cbor-napi` decisive
# test) that a blanket forbid cannot accommodate. Each entry here must
# carry `[lints.rust] unsafe_code = "allow"` instead of the inherited form
# — checked below, not just asserted by this comment.
EXEMPT_MANUAL_OVERRIDE = {
    "crates/cratestack-cbor-napi",
    "examples/embedded-expo/native",
    "examples/react-nextjs-daisyui/napi",
    # flutter_rust_bridge's generated `frb_generated.rs` (gitignored;
    # produced by `just frb-generate examples/embedded-flutter`) crosses a
    # raw Dart<->Rust FFI boundary and cannot compile under the workspace
    # `unsafe_code = "forbid"`. Same category as the three above, and the
    # reason this crate previously could not be built at all — see #600.
    "examples/embedded-flutter/native",
}


def relpath(manifest_path: str, root: Path) -> str:
    return str(Path(manifest_path).resolve().parent.relative_to(root))


def load_toml(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def has_workspace_lints_optin(data: dict) -> bool:
    return data.get("lints", {}).get("workspace") is True


def has_manual_unsafe_allow(data: dict) -> bool:
    return data.get("lints", {}).get("rust", {}).get("unsafe_code") == "allow"


def check_member(rel: str, manifest_path: Path, errors: list[str]) -> None:
    data = load_toml(manifest_path)
    if rel in EXEMPT_MANUAL_OVERRIDE:
        if not has_manual_unsafe_allow(data):
            errors.append(
                f"{rel}/Cargo.toml: on the FFI exemption list but missing "
                f"the required `[lints.rust]\\nunsafe_code = \"allow\"` "
                f"manual override"
            )
        if has_workspace_lints_optin(data):
            errors.append(
                f"{rel}/Cargo.toml: has BOTH the FFI manual override "
                f"exemption entry AND `[lints]\\nworkspace = true` — Cargo "
                f"rejects this combination; pick one (see this crate's own "
                f"manifest comment)"
            )
        return
    if not has_workspace_lints_optin(data):
        errors.append(
            f"{rel}/Cargo.toml: missing `[lints]\\nworkspace = true` — "
            f"`[workspace.lints.rust] unsafe_code = \"forbid\"` is inert "
            f"for this crate (cratestack#523). If this crate has a genuine "
            f"FFI boundary that needs `unsafe`, add it to "
            f"EXEMPT_MANUAL_OVERRIDE in .ci/lints_workspace_check.py "
            f"alongside a `[lints.rust] unsafe_code = \"allow\"` override "
            f"and a comment explaining why."
        )


def check_standalone_workspace(rel: str, manifest_path: Path, errors: list[str]) -> None:
    data = load_toml(manifest_path)
    forbid = (
        data.get("workspace", {}).get("lints", {}).get("rust", {}).get("unsafe_code")
    )
    if forbid != "forbid":
        errors.append(
            f"{rel}/Cargo.toml: excluded from the root workspace (its own "
            f"`[workspace]` root) but missing its own "
            f"`[workspace.lints.rust] unsafe_code = \"forbid\"` declaration "
            f"(cratestack#523) — the root workspace's forbid does not "
            f"reach a disjoint workspace"
        )
    if not has_workspace_lints_optin(data):
        errors.append(
            f"{rel}/Cargo.toml: excluded from the root workspace but "
            f"missing `[lints]\\nworkspace = true` to opt into its own "
            f"local `[workspace.lints]` table (cratestack#523)"
        )


def main() -> int:
    root = Path(".").resolve()
    metadata = json.load(sys.stdin)
    errors: list[str] = []

    for pkg in metadata["packages"]:
        manifest_path = Path(pkg["manifest_path"])
        try:
            rel = relpath(pkg["manifest_path"], root)
        except ValueError:
            continue  # not under this repo root; not our concern
        check_member(rel, manifest_path, errors)

    root_manifest = load_toml(root / "Cargo.toml")
    excluded = root_manifest.get("workspace", {}).get("exclude", [])
    for rel in excluded:
        manifest_path = root / rel / "Cargo.toml"
        if not manifest_path.is_file():
            errors.append(
                f"{rel}: listed in root `[workspace] exclude` but has no "
                f"Cargo.toml — update this check or the exclude list"
            )
            continue
        check_standalone_workspace(rel, manifest_path, errors)

    if errors:
        for e in errors:
            print(f"::error::{e}", file=sys.stderr)
        print(f"\n{len(errors)} manifest(s) failed the lints opt-in check.", file=sys.stderr)
        return 1

    member_count = len(metadata["packages"])
    print(
        f"OK: {member_count} root workspace member(s) + {len(excluded)} "
        f"excluded standalone workspace(s) all comply."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
