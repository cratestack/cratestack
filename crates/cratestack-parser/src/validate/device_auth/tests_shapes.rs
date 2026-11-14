//! `@device_auth`'s shape rules: the fields each leg maps, by role, and the
//! position of the error (always the marker).

use super::tests_support::{BASE, broken, refused};

#[test]
fn the_argument_is_one_plain_type() {
    broken(
        "beginRenewal",
        "beginRenewal(args: BeginRenewalArgs)",
        "beginRenewal(a: BeginRenewalArgs, b: Revoked)",
        "takes exactly one argument, a `type`; this has 2",
    );
    broken(
        "beginRenewal",
        "beginRenewal(args: BeginRenewalArgs)",
        "beginRenewal()",
        "takes exactly one argument, a `type`; this has 0",
    );
    broken(
        "beginRenewal",
        "beginRenewal(args: BeginRenewalArgs)",
        "beginRenewal(args: String)",
        "its argument `args` must be a `type` declaration",
    );
    broken(
        "beginRenewal",
        "beginRenewal(args: BeginRenewalArgs)",
        "beginRenewal(args: BeginRenewalArgs[])",
        "its argument `args` must be a `type` declaration",
    );
    broken(
        "beginRenewal",
        "beginRenewal(args: BeginRenewalArgs)",
        "beginRenewal(args: BeginRenewalArgs?)",
        "its argument `args` must be a `type` declaration",
    );
    let model = format!("{BASE}\nmodel Device {{\n  id Int @id\n}}\n")
        .replace("datasource db {\n  provider = \"none\"\n}\n", "")
        .replace(
            "beginRenewal(args: BeginRenewalArgs)",
            "beginRenewal(args: Device)",
        );
    refused(&model, "must be a `type` declaration (not a model");
}

#[test]
fn the_return_is_one_plain_type() {
    broken(
        "beginRenewal",
        "): RenewalChallenge\n",
        "): RenewalChallenge[]\n",
        "it must return a `type` declaration",
    );
    broken(
        "beginRenewal",
        "): RenewalChallenge\n",
        "): RenewalChallenge?\n",
        "it must return a `type` declaration",
    );
    broken(
        "confirmRenewal",
        "): Renewed\n",
        "): String\n",
        "it must return a `type` declaration",
    );
}

#[test]
fn begin_enrolment_needs_its_keys() {
    broken(
        "beginEnrolment",
        "  ambient Jwk\n",
        "",
        "has no `ambient`; it needs a required JWK",
    );
    broken(
        "beginEnrolment",
        "  ambient Jwk\n",
        "  ambient String\n",
        "`ambient` of its argument type `BeginEnrolmentArgs` must be a required JWK",
    );
    broken(
        "beginEnrolment",
        "  userPresent Jwk?\n",
        "  userPresent Jwk\n",
        "`userPresent` of its argument type `BeginEnrolmentArgs` must be an optional JWK",
    );
    broken(
        "beginEnrolment",
        "  kty String\n",
        "",
        "must be a required JWK `type` (`kty`, `crv`, `x`, `y`",
    );
    broken(
        "beginEnrolment",
        "  y String\n}",
        "  y String\n  d String\n}",
        "must be a required JWK `type`",
    );
    broken(
        "beginEnrolment",
        "  y String\n}",
        "  y Int\n}",
        "must be a required JWK `type`",
    );
}

#[test]
fn begin_enrolments_answer_has_a_challenge_per_key() {
    broken(
        "beginEnrolment",
        "  enrolmentId String\n  ambient String\n  userPresent String?\n}\n\ntype ConfirmEnrolmentArgs",
        "  enrolmentId String\n  userPresent String?\n}\n\ntype ConfirmEnrolmentArgs",
        "its return type `EnrolmentChallenges` has no `ambient`",
    );
    broken(
        "beginEnrolment",
        "  enrolmentId String\n  ambient String\n  userPresent String?\n}\n\ntype ConfirmEnrolmentArgs",
        "  enrolmentId String\n  ambient String\n}\n\ntype ConfirmEnrolmentArgs",
        "its return type `EnrolmentChallenges` has no `userPresent`",
    );
}

#[test]
fn user_present_is_in_every_enrolment_shape_or_none() {
    let none = BASE.replace("  userPresent Jwk?\n", "");
    // The challenge, the confirm input and the attestations would all drop it.
    refused(
        &none,
        "has `userPresent`, but `beginEnrolment`'s argument has none",
    );
    let partial = none.replace("  userPresent String?\n", "").replacen(
        "type EnrolledDevice {\n  deviceId String\n  userId String\n  phone String\n  ambient String\n",
        "type EnrolledDevice {\n  deviceId String\n  userId String\n  phone String\n  ambient String\n  userPresent String?\n",
        1,
    );
    refused(
        &partial,
        "its return type `EnrolledDevice` has `userPresent`",
    );
    let missing = BASE.replacen("type ConfirmEnrolmentArgs {\n  enrolmentId String\n  ambient String\n  userPresent String?\n}", "type ConfirmEnrolmentArgs {\n  enrolmentId String\n  ambient String\n}", 1);
    refused(
        &missing,
        "its argument type `ConfirmEnrolmentArgs` has no `userPresent`",
    );
}

#[test]
fn the_apps_own_fields_are_text() {
    broken(
        "beginEnrolment",
        "  phone String\n  otp String\n  ambient Jwk",
        "  phone Int\n  otp String\n  ambient Jwk",
        "`phone` of its argument type `BeginEnrolmentArgs` is the app's own field, which is carried as text",
    );
    broken(
        "confirmEnrolment",
        "  phone String\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs",
        "  phone Int\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs",
        "`phone` of its return type `EnrolledDevice` is the app's own field",
    );
}
