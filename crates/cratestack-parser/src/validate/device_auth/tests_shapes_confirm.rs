//! `@device_auth`'s shape rules for `confirmEnrolment`.

use super::tests_support::{BASE, broken, parse, refused};

#[test]
fn confirm_enrolments_other_inputs_must_come_back_from_leg_one() {
    broken(
        "confirmEnrolment",
        "type ConfirmEnrolmentArgs {\n  enrolmentId String\n",
        "type ConfirmEnrolmentArgs {\n  enrolmentId String\n  stray String\n",
        "`stray` of its argument type `ConfirmEnrolmentArgs` has no source",
    );
    // Same name, different type: not the same value.
    broken(
        "confirmEnrolment",
        "type ConfirmEnrolmentArgs {\n  enrolmentId String\n",
        "type ConfirmEnrolmentArgs {\n  enrolmentId String?\n",
        "`enrolmentId` of its argument type `ConfirmEnrolmentArgs` has no source",
    );
}

#[test]
fn confirm_enrolments_answer_has_the_device_and_its_user() {
    for field in ["deviceId", "userId", "ambient"] {
        let needle = "type EnrolledDevice {\n  deviceId String\n  userId String\n  phone String\n  ambient String\n".to_owned();
        let replaced = needle.replace(&format!("  {field} String\n"), "");
        broken(
            "confirmEnrolment",
            &needle,
            &replaced,
            &format!("its return type `EnrolledDevice` has no `{field}`"),
        );
    }
    broken(
        "confirmEnrolment",
        "  phone String\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs",
        "  name Int\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs",
        "`name` of its return type `EnrolledDevice` must be `String` or `String?`",
    );
    // name and email may be optional or required.
    parse(&BASE.replace("  phone String\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs", "  name String\n  email String?\n  ambient String\n  userPresent String?\n}\n\ntype BeginRenewalArgs")).expect("name and email");
}

#[test]
fn a_proof_field_must_be_text() {
    for (ty, shown) in [
        ("Int", "`Int`"),
        ("String[]", "`String[]`"),
        ("Boolean?", "`Boolean?`"),
    ] {
        let source = BASE
            .replace(
                "  phone String\n  otp String\n  ambient Jwk",
                "  phone String\n  ambient Jwk",
            )
            .replace(
                "type ConfirmEnrolmentArgs {\n  enrolmentId String\n",
                &format!("type ConfirmEnrolmentArgs {{\n  enrolmentId String\n  otp {ty}\n"),
            )
            .replace(
                r#"@device_auth(role: "confirmEnrolment")"#,
                r#"@device_auth(role: "confirmEnrolment", proof: "otp")"#,
            );
        refused(&source, &format!("is {shown} in its argument type"));
    }
}
