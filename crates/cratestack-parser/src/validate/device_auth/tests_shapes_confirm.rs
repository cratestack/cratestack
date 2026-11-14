//! `@device_auth`'s shape rules for `confirmEnrolment`.

use super::tests_support::{BASE, broken, parse};

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
        let needle = format!(
            "type EnrolledDevice {{\n  deviceId String\n  userId String\n  phone String\n  ambient String\n"
        );
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
