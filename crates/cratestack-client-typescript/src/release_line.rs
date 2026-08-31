//! Composes the version requirements a generated TypeScript client
//! declares, from a hand-verified lower bound and a **derived** upper
//! bound. The npm half of
//! `crates/cratestack-client-dart/src/release_line.rs`.
//!
//! # Why this is duplicated rather than shared
//!
//! The arithmetic is identical and the doctrine is identical, so a shared
//! helper looks obvious. It is deliberately not one. There is no crate
//! both client generators depend on that this would belong to:
//! `cratestack-core` is the only shared dependency, and it holds runtime
//! metadata — auth context, codec, envelope types — not codegen concerns.
//! Putting a version-range helper there inverts what `docs/adr/layers.toml`
//! (ADR 0014, CI-enforced) says the layer is for, to save twelve lines.
//! `package_floors.rs` already splits the same way for the same reason,
//! and says so.
//!
//! # What differs from the Dart half
//!
//! **Nothing about the ranges.** npm resolves `^0.8.15` on a `0.x` version
//! as `>=0.8.15 <0.9.0` — the second component is the breaking one pre-1.0,
//! exactly as pub does — so the caret this replaces had exactly the same
//! defect: once 0.9.0 shipped, a generated client kept refusing every
//! release a user could actually install.
//!
//! **The quoting hazard does not exist here.** A generated
//! `package.json` is JSON, where every value is quoted by construction. The
//! Dart side had to change both pubspec templates to quote the
//! interpolation, because an unquoted leading `>` is YAML's
//! folded-block-scalar indicator. There is no equivalent trap in JSON, so
//! this side needed no template change.
//!
//! **Both floors here widen, where the Dart ones mostly did not.**
//! `CRATESTACK_REFINE_FLOOR` was `^0.8.0` (`<0.9.0`) and
//! `CRATESTACK_CBOR_FLOOR` was `^0.8.15` (`<0.9.0`); both now reach
//! `<0.10.0`, so a generated client can resolve the 0.9.x releases npm has
//! carried since the 0.9.0 bump. Checked against the registry rather than
//! assumed: `@cratestack/refine` and `@cratestack/cbor` both publish
//! 0.9.1 through 0.9.4.

/// The exclusive upper bound for `version`'s release line: the next minor.
///
/// `0.9.4 -> "0.10.0"`, `0.10.2 -> "0.11.0"`, `1.2.3 -> "1.3.0"`.
///
/// Panics rather than returning an `Option`: the only caller passes
/// `CARGO_PKG_VERSION`, so a failure means the crate's own version is
/// unparseable, and a named panic beats a silently wrong constraint
/// reaching a user's `npm install`.
pub(crate) fn ceiling(version: &str) -> String {
    let mut parts = version.split('.');
    let mut component = |which: &str| -> u64 {
        let raw = parts
            .next()
            .unwrap_or_else(|| panic!("version {version:?} has no {which} component"));
        // Tolerates a pre-release or build suffix (`0.9.4-rc1`), which
        // `just bump` does not currently produce but semver permits.
        let digits: String = raw.chars().take_while(char::is_ascii_digit).collect();
        digits
            .parse()
            .unwrap_or_else(|error| panic!("version {version:?}'s {which} component: {error}"))
    };
    let major = component("major");
    let minor = component("minor");
    format!("{major}.{}.0", minor + 1)
}

/// `(floor, version) -> ">={floor} <{ceiling}"`.
///
/// npm accepts a space-separated range as an implicit AND, so this is one
/// constraint and not two — the same string shape pub takes.
pub(crate) fn requirement(floor: &str, version: &str) -> String {
    format!(">={floor} <{}", ceiling(version))
}

#[cfg(test)]
#[path = "release_line_tests.rs"]
mod release_line_tests;
