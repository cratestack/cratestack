//! Composes the version requirements a generated Dart client declares,
//! from a hand-verified lower bound and a **derived** upper bound.
//!
//! # The split, and why it is not one rule
//!
//! `package_floors.rs` states the rule these constants obey: a generated
//! dependency constraint is an API compatibility requirement, never a
//! function of `CARGO_PKG_VERSION`. That rule is about the **floor**, and
//! it stays absolute — a floor says "this release is the first that
//! carries what the generator emits", which is a fact about published
//! archives that no arithmetic can derive.
//!
//! The **ceiling** is a different question with a mechanical answer. It
//! exists to stop a generated client resolving across a pre-1.0 minor
//! whose API may have broken, and the boundary it wants is always "the
//! line after the one this generator was built from". A caret already
//! encoded that, badly: `^0.8.15` means `>=0.8.15 <0.9.0`, so the moment
//! 0.9.0 shipped, every generated client refused the only releases a user
//! could still get. That is cratestack#838 — a resolution failure, not a
//! compatibility one:
//!
//! ```text
//! So, because <app> depends on cratestack_cbor ^0.9.1, version solving failed.
//! ```
//!
//! Closing it by hand means editing five constants across two crates at
//! every minor bump, forever, and #838 is the proof that step gets
//! missed. So the ceiling follows the release line automatically and the
//! floor does not move at all.
//!
//! # What this costs, stated rather than buried
//!
//! Generator output is now a function of the release version again, in
//! one component. cratestack#754 decoupled them deliberately, so this is
//! a real reversal of part of that decision and worth naming:
//! `just bump` no longer leaves the committed snapshots and
//! `examples/flutter-riverpod/client` byte-identical across a **minor**
//! bump. It still does across a patch bump, which is the common case.
//!
//! The failure #754 actually cared about does not come back. That one was
//! a floor naming a version the registry could not serve yet — an
//! unresolvable constraint. A ceiling names an exclusive upper bound that
//! is *supposed* not to exist yet; `<0.10.0` resolves perfectly well
//! against a registry whose newest release is 0.9.4. The two are not the
//! same defect wearing different hats.

/// The exclusive upper bound for `version`'s release line: the next minor.
///
/// `0.9.4 -> "0.10.0"`, `0.10.2 -> "0.11.0"`, `1.2.3 -> "1.3.0"`.
///
/// Pre-1.0 this is the meaningful boundary, because pub treats the second
/// component as the breaking one (`^0.9.0` is `>=0.9.0 <0.10.0`). Post-1.0
/// it is narrower than a caret would be — deliberately conservative rather
/// than clever, since this repo is pre-1.0 and a 1.x rule should be
/// chosen when there is something real to test it against, not guessed
/// now.
///
/// Panics rather than returning an `Option`: the only caller passes
/// `CARGO_PKG_VERSION`, so a failure here means the crate's own version is
/// unparseable, and a named panic beats a silently wrong constraint
/// reaching a user's `pub get`.
pub(crate) fn ceiling(version: &str) -> String {
    let mut parts = version.split('.');
    let mut component = |which: &str| -> u64 {
        let raw = parts
            .next()
            .unwrap_or_else(|| panic!("version {version:?} has no {which} component"));
        // Tolerates a pre-release or build suffix on the last component
        // read (`0.9.4-rc1`), which `just bump` does not currently produce
        // but semver permits.
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
/// The emitted string is always a range, never a caret, and that is not
/// only about the derived ceiling: `CRATESTACK_ANNOTATIONS_FLOOR`'s floor
/// and ceiling sit on different minors (`>=0.8.10 <0.10.0`), which no
/// caret can express. See `package_floors.rs` for that one.
///
/// **Callers must emit this quoted.** In YAML a leading `>` is the
/// folded-block-scalar indicator and `>=` is not a valid header, so an
/// unquoted range is a hard `ScannerError` at the consumer's `pub get`
/// rather than a value that parses wrongly — both pubspec templates quote
/// every floor for this reason.
pub(crate) fn requirement(floor: &str, version: &str) -> String {
    format!(">={floor} <{}", ceiling(version))
}

#[cfg(test)]
#[path = "release_line_tests.rs"]
mod release_line_tests;
