//! Guard #1 for cratestack#779's API floors: the offline half, mirroring
//! `cratestack-client-dart/src/package_floors_tests.rs`. Guard #2 (a real
//! `npm install` + typecheck at the exact floors) lives in CI, because
//! only the registry can say whether a version was actually published —
//! the class of defect #754 found when a hand-written `^0.8.8` floor
//! turned out to name a version that never existed.
//!
//! A unit-test module rather than `tests/`, matching this workspace's
//! existing `*_tests.rs` convention, so the constants stay `pub(crate)`
//! instead of being widened into the public API just to be asserted on.

use super::{CRATESTACK_CBOR_FLOOR, CRATESTACK_REFINE_FLOOR};

/// Every floor this module guards, paired with the constant's name so a
/// failure names the thing to edit rather than just its value.
const FLOORS: [(&str, &str); 2] = [
    ("CRATESTACK_REFINE_FLOOR", CRATESTACK_REFINE_FLOOR),
    ("CRATESTACK_CBOR_FLOOR", CRATESTACK_CBOR_FLOOR),
];

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

/// A floor at or above the *current* workspace version is unresolvable
/// for exactly the reason #779 exists: `just bump` moves this crate's
/// `CARGO_PKG_VERSION` before the tag that publishes the npm packages,
/// so the current version is by definition not on the registry yet on a
/// bump PR. Requiring the floor to be **strictly below** it encodes two
/// things at once —
///
/// 1. the floor names a release that has already shipped, and
/// 2. the floor is not tracking the release version,
///
/// — the second being the property that *is* the fix, and the one a
/// well-meaning "keep it in sync with the bump" change would quietly
/// undo.
///
/// Deliberately not a claim that the floor was actually published: npm
/// is the only authority for that, and it is CI's install-at-the-floor
/// step that checks it.
#[test]
fn floors_are_below_the_current_unpublished_workspace_version() {
    let current = parse_caret(&format!("^{}", env!("CARGO_PKG_VERSION")));
    for (name, floor) in FLOORS {
        assert!(
            parse_caret(floor) < current,
            "{name} is {floor}, but this crate is at {current:?} — a floor at or above the \
             current version names something npm cannot serve until the release tag is pushed, \
             which is cratestack#779 itself. Floors are API-compatibility constants; they must \
             not follow `just bump`."
        );
    }
}
