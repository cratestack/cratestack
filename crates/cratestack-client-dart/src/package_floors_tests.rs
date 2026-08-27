//! Guard #1 for cratestack#754's API floors: the numbers in
//! [`super`] are *derived-checkable* against files already in this repo,
//! so the two cannot drift apart silently the way the hand-maintained
//! `^0.8.8` did (see [`super`]'s module doc for that receipt).
//!
//! A unit-test module rather than `tests/`, matching this workspace's
//! existing `*_tests.rs` convention, so the constants stay `pub(crate)`
//! instead of being widened into the public API just to be asserted on.

use super::{CRATESTACK_ANNOTATIONS_FLOOR, CRATESTACK_BUILDER_FLOOR};

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

/// A floor at or above the *current* workspace version is unresolvable
/// for exactly the reason #754 exists: `just bump` moves
/// `dart-packages/*/pubspec.yaml` (and this crate's `CARGO_PKG_VERSION`)
/// before the tag that publishes them, so the current version is by
/// definition not on pub.dev yet on a bump PR. Requiring the floor to be
/// **strictly below** it therefore encodes two things at once —
///
/// 1. the floor names a release that has already shipped, and
/// 2. the floor is not tracking the release version,
///
/// — the second being the property that *is* the fix, and the one a
/// well-meaning "keep it in sync with the bump" change would quietly
/// undo.
///
/// Deliberately not a claim that the floor was actually published:
/// pub.dev is the only authority for that, and the previous `^0.8.8`
/// floor named a version that never existed while satisfying every
/// offline check available. CI's `flutter (flutter-riverpod example)`
/// job, resolving at the exact floor, is what catches that class (guard
/// #2 — see [`super`]'s module doc).
///
/// **Known limitation, stated rather than hidden:** if the annotation
/// surface ever changes *within* the release being cut, the honest floor
/// would be that unpublished release and this test would fail. That is a
/// real chicken-and-egg in the lockstep publishing model, not a flaw in
/// the assertion — the fix is to publish the annotation package first,
/// which is what the un-lockstepping follow-up on #754 is about.
#[test]
fn floors_are_below_the_current_unpublished_workspace_version() {
    for (package, floor) in [
        ("cratestack_annotations", CRATESTACK_ANNOTATIONS_FLOOR),
        ("cratestack_builder", CRATESTACK_BUILDER_FLOOR),
    ] {
        let floor_parts = parse_caret(floor);
        let current = parse_caret(&format!("^{}", pubspec_value(package, "version")));
        assert!(
            floor_parts < current,
            "generated clients ask for {package} {floor}, but this repo's \
             dart-packages/{package}/pubspec.yaml is at {current:?} — a floor at or above the \
             current version names something pub.dev cannot serve until the release tag is \
             pushed, which is cratestack#754 itself. Floors are API-compatibility constants; they \
             must not follow `just bump`."
        );
    }
}
