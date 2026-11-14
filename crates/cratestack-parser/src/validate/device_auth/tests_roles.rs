//! `@device_auth`'s cross-procedure rules: each role once, the five
//! interface roles together, public legs public, `revoke` not.

use super::tests_support::{BASE, edit, line_of, parse, refused};

const BEGIN_RENEWAL: &str = r#"@device_auth(role: "beginRenewal")"#;
const REVOKE: &str = r#"@device_auth(role: "revoke")"#;

#[test]
fn a_duplicate_role_is_refused_at_the_second_with_the_first_named() {
    let source = edit(
        "  @device_auth(role: \"revoke\")\n",
        "  @device_auth(role: \"beginRenewal\")\n",
    );
    let source = source.replace(
        "mutation procedure beginRenewal(args: BeginRenewalArgs)",
        "mutation procedure beginRenewal(args: BeginRenewalArgs)",
    );
    let error = refused(
        &source,
        "`role: \"beginRenewal\"` is already on procedure `beginRenewal` (line",
    );
    let second = source.rfind(BEGIN_RENEWAL).expect("second marker");
    assert_eq!(error.line(), source[..second].matches('\n').count() + 1);
    assert_eq!(
        error.related().len(),
        1,
        "the first marker is a related span"
    );
    assert_eq!(error.related()[0].line(), line_of(&source, BEGIN_RENEWAL));
}

#[test]
fn every_missing_interface_role_is_named_at_the_first_marker() {
    for role in [
        "beginEnrolment",
        "confirmEnrolment",
        "beginRenewal",
        "confirmRenewal",
        "revoke",
    ] {
        let marker = format!("  @device_auth(role: \"{role}\")\n");
        let source = edit(&marker, "");
        let error = refused(
            &source,
            &format!("but not `{role}`: the device roles are all-or-nothing"),
        );
        assert_eq!(error.line(), line_of(&source, "@device_auth"), "{role}");
    }
}

#[test]
fn several_missing_roles_are_listed_together() {
    let only_begin = BASE
        .replace("  @device_auth(role: \"confirmEnrolment\")\n", "")
        .replace("  @device_auth(role: \"confirmRenewal\")\n", "")
        .replace("  @device_auth(role: \"revoke\")\n", "");
    refused(
        &only_begin,
        "but not `confirmEnrolment`, `confirmRenewal`, `revoke`",
    );
}

#[test]
fn the_optional_leg_alone_is_not_a_device_auth_schema() {
    let only_code = BASE
        .replace("  @device_auth(role: \"beginEnrolment\")\n", "")
        .replace("  @device_auth(role: \"confirmEnrolment\")\n", "")
        .replace("  @device_auth(role: \"beginRenewal\")\n", "")
        .replace("  @device_auth(role: \"confirmRenewal\")\n", "")
        .replace("  @device_auth(role: \"revoke\")\n", "");
    refused(
        &only_code,
        "marks `sendEnrolmentCode` but not `beginEnrolment`, `confirmEnrolment`, `beginRenewal`, \
         `confirmRenewal`, `revoke`",
    );
}

#[test]
fn a_schema_with_no_marker_is_untouched() {
    parse(&BASE.replace("  @device_auth", "  // @device_auth")).expect("no markers, no rules");
}

#[test]
fn a_public_leg_must_be_exactly_allow_true() {
    for (policy, found) in [
        ("@allow(auth() != null)", "it writes @allow(auth() != null)"),
        (
            "@allow(true)\n  @deny(auth() == null)",
            "it writes @allow(true) @deny(auth() == null)",
        ),
        (
            "@allow(true)\n  @authn(maxAge: 60)",
            "it writes @allow(true) @authn(maxAge: 60)",
        ),
        (
            "@allow(true)\n  @allow(true)",
            "it writes @allow(true) @allow(true)",
        ),
    ] {
        let source = edit(
            "  @device_auth(role: \"beginRenewal\")\n  @allow(true)\n",
            &format!("  @device_auth(role: \"beginRenewal\")\n  {policy}\n"),
        );
        let error = refused(
            &source,
            "so the procedure must be public: exactly `@allow(true)`",
        );
        assert!(
            error.message().contains(found),
            "{policy}: {}",
            error.message()
        );
        assert_eq!(error.line(), line_of(&source, BEGIN_RENEWAL), "{policy}");
    }
    let none = edit(
        "  @device_auth(role: \"beginRenewal\")\n  @allow(true)\n",
        "  @device_auth(role: \"beginRenewal\")\n",
    );
    refused(&none, "(it has no policy)");
}

#[test]
fn allow_true_is_read_whatever_its_spacing() {
    let spaced = edit(
        "  @device_auth(role: \"beginRenewal\")\n  @allow(true)\n",
        "  @device_auth(role: \"beginRenewal\")\n  @allow( true )\n",
    );
    parse(&spaced).expect("`@allow( true )` is public too");
}

#[test]
fn a_public_revoke_is_a_hard_error() {
    let source = edit(
        "  @device_auth(role: \"revoke\")\n  @allow(auth() != null)\n",
        "  @device_auth(role: \"revoke\")\n  @allow(true)\n",
    );
    let error = refused(
        &source,
        "`revoke` must not be public: anyone could revoke any device",
    );
    assert_eq!(error.line(), line_of(&source, REVOKE));
    let spaced = edit(
        "  @device_auth(role: \"revoke\")\n  @allow(auth() != null)\n",
        "  @device_auth(role: \"revoke\")\n  @allow( true )\n",
    );
    refused(&spaced, "`revoke` must not be public");
}
