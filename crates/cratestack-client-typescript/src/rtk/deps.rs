//! `--rtk`'s (issue #906) own `package.json.j2` `peerDependencies`/
//! `devDependencies` entries — split out of `crate::package_deps` (which
//! calls straight into [`rtk_peer_dependencies`]/[`rtk_dev_dependencies`])
//! purely to keep that file under this repo's ~200-LoC convention; there
//! is no other reason this couldn't live inline there, the way `--refine`/
//! `--swr`/`--tanstack`'s own entries do.

use crate::config::TypeScriptGeneratorConfig;
use crate::package_deps::DependencyEntry;

const REACT_RANGE: &str = "^18.0.0 || ^19.0.0";

/// `@reduxjs/toolkit`'s range: `>=2.2.7 <3`. Two independent facts set the
/// floor, and it is the later of the two.
///
/// 1. **A `bigint` query argument (ADR 0019): 2.2.4.** A `BigInt` field is a
///    `bigint` in the generated client, so `get<Model>(id)` and a procedure's
///    `args` can hold one. The default `serializeQueryArgs` throws "Do not
///    know how to serialize a BigInt" before reduxjs/redux-toolkit@ae838b4c
///    ("remove replacer param in favor of handling bigints inside of
///    defaultSerializeQueryArgs", 2024-04-08, PR #4315), and `v2.2.4`
///    (published 2024-05-09) is the first tag containing it: GitHub's compare
///    API reports `ae838b4c` as an ancestor of `v2.2.4` and not of `v2.2.3`.
///    RAN on the published tarballs: `defaultSerializeQueryArgs({ queryArgs:
///    2n ** 53n })` throws on `2.0.0`, `2.2.1`, `2.2.2` and `2.2.3`, and
///    returns `e({"$bigint":"9007199254740992"})` on `2.2.4`, `2.2.5` and
///    `2.13.0`.
/// 2. **A buildable `rtk-api.ts`: 2.2.7.** `createCratestackRtkApi`'s return
///    type is inferred, so `declaration: true` has to write it into a `.d.ts`,
///    which fails with `TS2527: The inferred type of 'createCratestackRtkApi'
///    references an inaccessible 'unique symbol' type` on every release before
///    `2.2.7` (RAN: `npm run build` of a generated package, TypeScript 7, fails
///    on 2.0.0, 2.2.3, 2.2.4, 2.2.5 and 2.2.6 and passes on 2.2.7, 2.2.8, 2.3.0
///    and 2.4.0). `2.2.7`'s release notes say why: it exports the unique
///    symbols RTK's types use internally ("TS type portability"). The old
///    `^2.0.0` floor could never have built; nothing resolved it because `npm
///    install` takes the newest 2.x.
///
/// `tests/bigint_query_keys.rs` re-pins the installed toolkit to whatever this
/// constant declares, builds the generated package and runs a bigint-argument
/// hook against it, so lowering the floor fails there.
///
/// The toolkit's own `react` peer range gains `^19` only in 2.5.0 (RAN:
/// `npm view @reduxjs/toolkit@2.2.7 peerDependencies.react` is `^16.9.0 ||
/// ^17.0.0 || ^18`), so a React 19 app that pins the toolkit below 2.5 hits an
/// npm peer conflict. The caret range resolves to the newest 2.x, which is
/// fine for it; this is not a reason to raise the floor.
const REDUX_TOOLKIT_RANGE: &str = "^2.2.7";

/// `react`/`react-redux`/`@reduxjs/toolkit`, plus `@cratestack/adapter-rtk`
/// when `rtk_adapter_version_requirement` is non-empty (RPC transport
/// only — see `crate::rtk`'s module doc). Empty when `--rtk` is off.
///
/// `react` is omitted here when `config.swr` is also on: `--swr` already
/// pushes an identical `react` entry, and a second entry of the same name
/// would render as a duplicate JSON *key* — valid JSON, but a landmine
/// (a `Record`-typed reader sees only the second; a human reviewing the
/// diff sees two different-looking promises for the same package).
pub(crate) fn rtk_peer_dependencies(
    config: &TypeScriptGeneratorConfig,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    if !config.rtk {
        return Vec::new();
    }
    let mut deps = Vec::new();
    if !config.swr {
        deps.push(DependencyEntry::new("react", REACT_RANGE.to_owned()));
    }
    deps.push(DependencyEntry::new("react-redux", "^9.0.0".to_owned()));
    deps.push(DependencyEntry::new(
        "@reduxjs/toolkit",
        REDUX_TOOLKIT_RANGE.to_owned(),
    ));
    if !rtk_adapter_version_requirement.is_empty() {
        deps.push(DependencyEntry::new(
            "@cratestack/adapter-rtk",
            rtk_adapter_version_requirement.to_owned(),
        ));
    }
    deps
}

/// Same entries as [`rtk_peer_dependencies`] plus `@types/react` — the
/// dev-only type declarations a peer dependency never carries, following
/// `--swr`'s existing split between the two lists for the same package.
pub(crate) fn rtk_dev_dependencies(
    config: &TypeScriptGeneratorConfig,
    rtk_adapter_version_requirement: &str,
) -> Vec<DependencyEntry> {
    if !config.rtk {
        return Vec::new();
    }
    let mut deps = Vec::new();
    if !config.swr {
        deps.push(DependencyEntry::new("@types/react", REACT_RANGE.to_owned()));
        deps.push(DependencyEntry::new("react", REACT_RANGE.to_owned()));
    }
    deps.push(DependencyEntry::new("react-redux", "^9.0.0".to_owned()));
    deps.push(DependencyEntry::new(
        "@reduxjs/toolkit",
        REDUX_TOOLKIT_RANGE.to_owned(),
    ));
    if !rtk_adapter_version_requirement.is_empty() {
        deps.push(DependencyEntry::new(
            "@cratestack/adapter-rtk",
            rtk_adapter_version_requirement.to_owned(),
        ));
    }
    deps
}
