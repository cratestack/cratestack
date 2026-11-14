//! `@device_auth`'s shape rules for the renewal legs, `revoke` and the
//! optional leg (the enrolment legs are in `tests_shapes`).

use super::tests_support::{BASE, broken, parse};

#[test]
fn begin_renewal_takes_exactly_the_device_and_the_role() {
    broken(
        "beginRenewal",
        "type BeginRenewalArgs {\n  deviceId String\n  role String\n}",
        "type BeginRenewalArgs {\n  deviceId String\n  role String\n  extra String\n}",
        "its argument type `BeginRenewalArgs` has `extra`, which has no source",
    );
    broken(
        "beginRenewal",
        "  deviceId String\n  role String\n}",
        "  deviceId String\n}",
        "has no `role`; it needs a required `String`",
    );
    broken(
        "beginRenewal",
        "  deviceId String\n  role String\n}",
        "  deviceId String\n  role Int\n}",
        "`role` of its argument type `BeginRenewalArgs` must be a required `String`, not `Int`",
    );
    broken(
        "beginRenewal",
        "  challengeId String\n  challenge String\n}",
        "  challengeId String\n}",
        "its return type `RenewalChallenge` has no `challenge`",
    );
}

#[test]
fn confirm_renewal_needs_the_signature_and_what_leg_one_answered() {
    broken(
        "confirmRenewal",
        "  challengeId String\n  signature String\n}",
        "  challengeId String\n}",
        "its argument type `ConfirmRenewalArgs` has no `signature`",
    );
    broken(
        "confirmRenewal",
        "  challengeId String\n  signature String\n}",
        "  challengeId String\n  signature String\n  other String\n}",
        "`other` of its argument type `ConfirmRenewalArgs` has no source",
    );
    broken(
        "confirmRenewal",
        "type Renewed {\n  attestation String\n}",
        "type Renewed {\n  jws String\n}",
        "its return type `Renewed` has no `attestation`",
    );
}

#[test]
fn revoke_takes_exactly_the_device_id() {
    broken(
        "revoke",
        "type RevokeArgs {\n  deviceId String\n}",
        "type RevokeArgs {\n  deviceId String\n  reason String\n}",
        "its argument type `RevokeArgs` has `reason`, which has no source",
    );
    broken(
        "revoke",
        "type RevokeArgs {\n  deviceId String\n}",
        "type RevokeArgs {\n  id String\n}",
        "its argument type `RevokeArgs` has no `deviceId`",
    );
}

#[test]
fn the_optional_leg_may_take_anything_and_return_anything() {
    parse(&BASE.replace(
        "sendEnrolmentCode(args: SendCodeArgs): CodeSent",
        "sendEnrolmentCode(phone: String, region: String?): String",
    ))
    .expect("any args, any return");
}
