//! Guard #1, part 2: the two checks that compare a floor against the
//! version of the package it names.
//!
//! Split out of `package_floors_tests.rs` only to stay under this
//! repo's 200-line file ceiling (`just verify-file-length`). Same
//! module, same `pub(crate)` visibility — `#[path]`-included by its
//! sibling rather than living in `tests/`, for the reason that file's
//! own header gives.

use super::super::{CRATESTACK_ANNOTATIONS_FLOOR, CRATESTACK_BUILDER_FLOOR, CRATESTACK_CBOR_FLOOR};
use super::{parse_caret, pubspec_value};

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
/// Every floor this module guards, in one place so the two tests below
/// cannot drift apart on which packages they cover.
const FLOORS: [(&str, &str); 3] = [
    ("cratestack_annotations", CRATESTACK_ANNOTATIONS_FLOOR),
    ("cratestack_builder", CRATESTACK_BUILDER_FLOOR),
    // cratestack#779: `cratestack_cbor` joins the same guard now that
    // it emits a floor rather than `^{CARGO_PKG_VERSION}`. Before
    // that it would have failed this test by construction, which is
    // the whole point of it being here.
    ("cratestack_cbor", CRATESTACK_CBOR_FLOOR),
];

/// A floor may EQUAL the in-repo package version, but only when that
/// version has already published to pub.dev, and only with the evidence
/// recorded here. Ported from `cratestack-client-typescript`, which grew
/// the same valve in #806 for the same shape of problem.
///
/// Why the strict `<` was not enough on its own: it is a conservative proxy
/// for "the floor names a shipped release" — correct on a bump PR, and
/// over-strict in the window between a release publishing and the next
/// bump. cratestack#838 landed squarely in that window. All three
/// `cratestack_*` Dart packages were live on pub.dev at 0.9.1 while this
/// workspace also read 0.9.1, and the old `^0.8.x` floors were excluding
/// them by caret ceiling — so refusing the equal case would have forced a
/// consumer-visible resolution failure to wait for an unrelated bump.
///
/// This is a safety valve, not a hole. An entry is a deliberate act with a
/// reason attached; `published_equal_floor_entries_are_still_needed`
/// deletes it the moment the ordinary rule covers it again; and it stays
/// deliberately offline, because pub.dev is the only authority on what
/// actually published and CI's `flutter (flutter-riverpod example)` job,
/// resolving at these exact floors, is what proves it.
/// Empty, and that is the mechanism working rather than an omission. All
/// three floors were raised to `^0.9.1` in the same change that added this
/// list (#838), at a moment when the in-repo packages also read 0.9.1 — so
/// every one of them needed an entry here. `dart-packages/*` then moved to
/// 0.9.2 (#837), which puts the floors strictly below the in-repo version
/// again, the ordinary `<` rule covers them, and
/// `published_equal_floor_entries_are_still_needed` required the entries to
/// be deleted. Leaving them would have silently widened the exemption to
/// whatever floor next lands on the current version.
///
/// The floors deliberately did NOT move to `^0.9.2` alongside that bump:
/// 0.9.2 is not published, and floors are API-compatibility constants that
/// must not follow `just bump`. `^0.9.1` names the newest release pub.dev
/// can actually serve.
const PUBLISHED_EQUAL_FLOORS: [(&str, &str); 0] = [];

#[test]
fn floors_are_below_the_current_unpublished_workspace_version() {
    for (package, floor) in FLOORS {
        let floor_parts = parse_caret(floor);
        let current = parse_caret(&format!("^{}", pubspec_value(package, "version")));
        if floor_parts == current {
            assert!(
                PUBLISHED_EQUAL_FLOORS
                    .iter()
                    .any(|(listed, _)| *listed == package),
                "generated clients ask for {package} {floor}, which EQUALS this repo's \
                 dart-packages/{package}/pubspec.yaml version {current:?}. That is allowed only \
                 for a floor whose release has already published — add it to \
                 PUBLISHED_EQUAL_FLOORS with the evidence, or leave the floor below. Floors are \
                 API-compatibility constants; they must not follow `just bump`."
            );
            continue;
        }
        assert!(
            floor_parts < current,
            "generated clients ask for {package} {floor}, but this repo's \
             dart-packages/{package}/pubspec.yaml is at {current:?} — a floor ABOVE the current \
             version names something pub.dev cannot serve under any circumstances, which is \
             cratestack#754 itself. Floors are API-compatibility constants; they must not follow \
             `just bump`."
        );
    }
}

/// The list above is only a safety valve if it cannot rot into a blanket
/// exemption. An entry that no longer equals the in-repo version has
/// outlived its purpose: the next `just bump` puts the floor genuinely
/// below the version, the ordinary rule covers it, and what is left is a
/// hole waiting for a future floor to fall through.
#[test]
fn published_equal_floor_entries_are_still_needed() {
    for (name, reason) in PUBLISHED_EQUAL_FLOORS {
        let floor = FLOORS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .unwrap_or_else(|| panic!("PUBLISHED_EQUAL_FLOORS names {name}, which is not a floor"))
            .1;
        assert_eq!(
            parse_caret(floor),
            parse_caret(&format!("^{}", pubspec_value(name, "version"))),
            "PUBLISHED_EQUAL_FLOORS still lists {name} ({floor}), but it no longer equals the \
             in-repo version — the ordinary rule now covers it. Delete the entry; leaving it in \
             place widens the exemption for whatever floor lands on this version next. Its \
             recorded reason was: {reason}"
        );
    }
}
