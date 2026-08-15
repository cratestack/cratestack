//! `package.json.j2`'s `peerDependencies`/`devDependencies` entry lists.
//!
//! Split out of `context.rs` (issue #617) to keep that file under this
//! repo's ~200-LoC convention as it grew a third optional dependency group
//! (`--tanstack`, alongside `--refine`/`--swr`) — this module is pure list-
//! building with no other context-assembly concerns, so it moves cleanly
//! on its own.
//!
//! Before issue #617, `@tanstack/react-query` was package.json.j2's last
//! *unconditional* peer/dev dependency entry, which gave every optional
//! `{% if refine %}`/`{% if swr %}` block ahead of it a safe place to hang
//! a trailing comma: whatever was on, `@tanstack/react-query` always
//! followed. Gating `--tanstack` too removes that anchor — `peerDependencies`
//! can now have zero, one, two, or three of {refine, swr, tanstack} present,
//! and "does this entry need a trailing comma" depends on which of the
//! *other* optional groups render after it, a combinatorial "join with
//! separator" problem nested `{% if %}` blocks don't solve cleanly. A
//! `{% for %}` loop with `loop.last` in the template solves it generically
//! instead, over the ordered lists this module builds.

use crate::config::TypeScriptGeneratorConfig;

/// One `"name": "version"` entry in `package.json`'s `peerDependencies` or
/// `devDependencies`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DependencyEntry {
    name: &'static str,
    version: String,
}

/// `package.json.j2`'s `peerDependencies`, in the same order the object
/// used to render in before issue #617: `--refine`'s two entries, then
/// `--swr`'s two, then `--tanstack`'s one (now optional like the other
/// two). Empty when none of the three flags are set — renders a valid
/// empty `"peerDependencies": {}`.
pub(crate) fn peer_dependencies_for(
    config: &TypeScriptGeneratorConfig,
    refine_version_requirement: &str,
) -> Vec<DependencyEntry> {
    let mut deps = Vec::new();
    if config.refine {
        deps.push(DependencyEntry {
            name: "@cratestack/refine",
            version: refine_version_requirement.to_owned(),
        });
        deps.push(DependencyEntry {
            name: "@refinedev/core",
            version: "^5.0.0".to_owned(),
        });
    }
    if config.swr {
        deps.push(DependencyEntry {
            name: "react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "swr",
            version: "^2.2.0".to_owned(),
        });
    }
    if config.tanstack {
        deps.push(DependencyEntry {
            name: "@tanstack/react-query",
            version: "^5.0.0".to_owned(),
        });
    }
    deps
}

/// `package.json.j2`'s `devDependencies` — same flag order as
/// `peer_dependencies_for`, plus the `typescript` entry every generated
/// package needs regardless of flags. `typescript` is appended here
/// (rather than left for the template to render unconditionally after the
/// `{% for %}` loop) so the template has exactly one rendering strategy
/// shared by both objects.
pub(crate) fn dev_dependencies_for(
    config: &TypeScriptGeneratorConfig,
    refine_version_requirement: &str,
) -> Vec<DependencyEntry> {
    let mut deps = Vec::new();
    if config.refine {
        deps.push(DependencyEntry {
            name: "@cratestack/refine",
            version: refine_version_requirement.to_owned(),
        });
        deps.push(DependencyEntry {
            name: "@refinedev/core",
            version: "^5.0.0".to_owned(),
        });
    }
    if config.swr {
        deps.push(DependencyEntry {
            name: "@types/react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "react",
            version: "^18.0.0 || ^19.0.0".to_owned(),
        });
        deps.push(DependencyEntry {
            name: "swr",
            version: "^2.2.0".to_owned(),
        });
    }
    if config.tanstack {
        deps.push(DependencyEntry {
            name: "@tanstack/react-query",
            version: "^5.0.0".to_owned(),
        });
    }
    deps.push(DependencyEntry {
        name: "typescript",
        version: "^7.0.2".to_owned(),
    });
    deps
}
