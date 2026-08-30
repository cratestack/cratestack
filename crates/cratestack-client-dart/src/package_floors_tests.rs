//! Guard #1 for cratestack#754's API floors: the numbers in
//! [`super`] are *derived-checkable* against files already in this repo,
//! so the two cannot drift apart silently the way the hand-maintained
//! `^0.8.8` did (see [`super`]'s module doc for that receipt).
//!
//! A unit-test module rather than `tests/`, matching this workspace's
//! existing `*_tests.rs` convention, so the constants stay `pub(crate)`
//! instead of being widened into the public API just to be asserted on.

use super::CRATESTACK_ANNOTATIONS_FLOOR;

/// `^X.Y.Z` -> `(X, Y, Z)`. Panics rather than returning an `Option`:
/// every caller here is a test whose failure message is more useful than
/// a `None`.
fn parse_caret(requirement: &str) -> (u64, u64, u64) {
    let digits = requirement
        .strip_prefix('^')
        .unwrap_or_else(|| panic!("expected a caret requirement, got {requirement:?}"));
    let mut parts = digits.split('.');
    let mut next = |which: &str| -> u64 {
        parts
            .next()
            .unwrap_or_else(|| panic!("{requirement:?} has no {which} component"))
            .parse()
            .unwrap_or_else(|error| panic!("{requirement:?}'s {which} component: {error}"))
    };
    (next("major"), next("minor"), next("patch"))
}

/// Reads a `key: value` pair off a pubspec, matching the first line whose
/// trimmed form starts with `{key}:`. Deliberately a dumb line scan and
/// not a YAML parse: these two pubspecs are hand-written, heavily
/// commented, and the values wanted here are plain scalars at a known
/// nesting depth — pulling in a YAML dependency for this would be the
/// larger risk.
fn pubspec_value(package: &str, key: &str) -> String {
    let path = format!(
        "{}/../../dart-packages/{package}/pubspec.yaml",
        env!("CARGO_MANIFEST_DIR")
    );
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {path}: {error}. Did the package move?"));
    let prefix = format!("{key}:");
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .unwrap_or_else(|| panic!("no `{key}:` line in {path}"))
        .to_owned()
}

/// The core derived check. A generated client lists `cratestack_builder`
/// under `dev_dependencies:` and `cratestack_annotations` under
/// `dependencies:`, and `build_runner` runs the former against
/// annotations emitted from the latter. If the generator's annotations
/// floor were *below* the floor the builder itself declares, pub could
/// resolve an annotation package the builder cannot read — the
/// `ConstantReader.read(...)`-throws-at-generation-time failure
/// `dart-packages/cratestack_builder/pubspec.yaml`'s own comment
/// describes, surfacing at a user's `build_runner` rather than at
/// `pub get`.
///
/// Reading the bound from that pubspec rather than restating it is the
/// whole point: raise it there and this test tells you to raise the
/// emitted floor too.
#[test]
fn emitted_annotations_floor_is_at_least_what_the_builder_requires() {
    let emitted = parse_caret(CRATESTACK_ANNOTATIONS_FLOOR);
    let required = parse_caret(&pubspec_value(
        "cratestack_builder",
        "cratestack_annotations",
    ));
    assert!(
        emitted >= required,
        "generated clients ask for cratestack_annotations {CRATESTACK_ANNOTATIONS_FLOOR}, but \
         dart-packages/cratestack_builder/pubspec.yaml requires at least {required:?} — pub could \
         resolve an annotation package the builder cannot read. Raise \
         CRATESTACK_ANNOTATIONS_FLOOR in src/package_floors.rs."
    );
}

#[path = "package_floors_tests/version_guards.rs"]
mod version_guards;
