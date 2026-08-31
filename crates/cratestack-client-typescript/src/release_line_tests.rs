//! Unit tests for the derived ceiling — the npm half of
//! `cratestack-client-dart/src/release_line_tests.rs`.
//!
//! Same reasoning for existing: these pin the arithmetic to a hand-written
//! table, which is what lets the integration tests compose a ceiling
//! instead of hardcoding one while keeping their hand-written *floor*
//! literals as tripwires (cratestack#779, #849).

use super::{ceiling, requirement};

/// Each row chosen for a reason: the current version, a two-digit minor
/// (what a naive string bump gets wrong), a tens boundary, the 0.0.x line,
/// and a post-1.0 version to pin the conservative behaviour deliberately.
#[test]
fn ceiling_is_the_next_minor() {
    for (version, expected) in [
        ("0.9.4", "0.10.0"),
        ("0.10.2", "0.11.0"),
        ("0.19.9", "0.20.0"),
        ("0.99.1", "0.100.0"),
        ("0.0.3", "0.1.0"),
        ("1.2.3", "1.3.0"),
    ] {
        assert_eq!(
            ceiling(version),
            expected,
            "ceiling({version:?}) should be {expected:?}"
        );
    }
}

/// `0.10.0` is the next minor this repo will cut, and the case a lexical
/// bump gets wrong. Separate so a regression names itself.
#[test]
fn the_two_digit_minor_does_not_regress_to_a_lower_line() {
    assert_eq!(ceiling("0.10.0"), "0.11.0");
    assert_ne!(
        ceiling("0.10.0"),
        "0.2.0",
        "a lexical bump of the minor would produce this"
    );
}

#[test]
fn a_prerelease_suffix_is_ignored() {
    assert_eq!(ceiling("0.9.4-rc1"), "0.10.0");
    assert_eq!(ceiling("0.9.4+build7"), "0.10.0");
}

/// Both npm floors sit on 0.8.x while the ceiling is now 0.10.0, so the
/// composed range spans two minors — the widening this change is for.
#[test]
fn requirement_pairs_the_floor_with_the_derived_ceiling() {
    assert_eq!(requirement("0.8.0", "0.9.4"), ">=0.8.0 <0.10.0");
    assert_eq!(requirement("0.8.15", "0.9.4"), ">=0.8.15 <0.10.0");
    assert_eq!(requirement("0.8.15", "0.10.0"), ">=0.8.15 <0.11.0");
}

/// A client generated at a version must be able to resolve that version's
/// own release line, or it could not use the packages it was generated
/// alongside.
#[test]
fn the_ceiling_always_exceeds_its_own_version() {
    let parsed = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap()
            })
            .collect()
    };
    for version in ["0.9.4", "0.10.2", "0.19.9", "0.0.3", "1.2.3"] {
        assert!(
            parsed(&ceiling(version)) > parsed(version),
            "ceiling({version:?}) = {:?} must exceed {version:?}",
            ceiling(version)
        );
    }
}

#[test]
#[should_panic(expected = "has no minor component")]
fn an_unparseable_version_panics_by_name() {
    ceiling("0");
}
