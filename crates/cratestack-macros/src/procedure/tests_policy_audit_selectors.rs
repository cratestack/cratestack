//! The generator's re-check of a policy attribute's invisible characters,
//! for the variation selector rule and the escaped error text
//! (GHSA-69g4-xvcm-vm2j). Like `tests_policy_audit`, these hand the IR to
//! the generator as a bypassed parser would.

use super::tests_policy_audit::{assert_refused, procedure_result, query_result, schema, with_raw};
use crate::policy::audit_model_policies;

const INVISIBLE: &str = "an invisible character";

/// A selector after `é`, after CJK or after an emoji renders like the plain
/// role, so in a policy the generator refuses it wherever it stands.
#[test]
fn a_variation_selector_in_a_policy_is_a_compile_error_after_any_character() {
    for role in [
        "banné\u{FE0F}",
        "管理者\u{FE0F}",
        "\u{2764}\u{FE0F}",
        "banné\u{E0100}",
    ] {
        let raw = format!("@deny(hasRole(\"{role}\"))");
        assert_refused(procedure_result(&raw), INVISIBLE);
        assert_refused(query_result(&raw), INVISIBLE);
    }
    let mut model = schema().models.remove(0);
    with_raw(
        &mut model.attributes,
        1,
        "@@deny(\"update\", auth().role == \"banné\u{FE0F}\")",
    );
    let message = audit_model_policies("model `Account`", &model, &["update"]).unwrap_err();
    assert!(message.contains("U+FE0F"), "{message}");
}

/// Without a selector the same non-ASCII roles generate as before.
#[test]
fn a_non_ascii_role_without_a_selector_generates() {
    for role in ["banné", "管理者", "\u{2764}"] {
        let raw = format!("@deny(hasRole(\"{role}\"))");
        procedure_result(&raw).expect("generates");
        query_result(&raw).expect("generates");
    }
}

/// The error quotes the attribute with NUL, BEL and ESC escaped, on both
/// of the re-check's refusals, so none reaches the compiler's output raw.
#[test]
fn the_refusal_never_quotes_a_control_character_raw() {
    for (raw, needle) in [
        ("@deny(hasRole(\"ban\u{0}ned\"))", "ban\\u{0}ned"),
        ("@deny(hasRole(\"ban\u{7}ned\"))", "ban\\u{7}ned"),
        ("@deny(hasRole(\"ban\u{1B}[8mned\"))", "ban\\u{1B}[8mned"),
        ("@Deny(hasRole(\"ban\u{7}ned\"))", "ban\\u{7}ned"),
        ("@deny(hasRole(\"banné\u{FE0F}\"))", "banné\\u{FE0F}"),
    ] {
        for result in [procedure_result(raw), query_result(raw)] {
            let message = result.expect_err(raw);
            assert!(message.contains(needle), "{message:?}");
            assert!(
                !message
                    .chars()
                    .any(|ch| ch.is_control() || ch == '\u{FE0F}'),
                "{message:?}"
            );
        }
    }
}
