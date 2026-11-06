use super::*;
use crate::value::Value;

const NOW: i64 = 1_800_000_000;

fn signed_in(auth_time: i64, amr: &[&str], acr: Option<&str>) -> CratestackContext {
    let mut builder = CratestackContext::builder("usr-1")
        .auth_time(auth_time)
        .amr(amr.iter().copied());
    if let Some(acr) = acr {
        builder = builder.acr(acr);
    }
    builder.build()
}

const FRESH_HWK: AuthnRequirement = AuthnRequirement {
    max_age: Some(300),
    amr: &["hwk", "user"],
    acr: &["aal2", "aal3"],
};

#[test]
fn a_requirement_with_nothing_is_met_by_anyone() {
    let none = AuthnRequirement::default();
    assert!(none.unmet(&CratestackContext::anonymous(), NOW).is_empty());
}

#[test]
fn a_fresh_strong_caller_meets_every_part() {
    let ctx = signed_in(NOW - 300, &["hwk", "user", "pin"], Some("aal3"));
    assert!(FRESH_HWK.unmet(&ctx, NOW).is_empty());
    assert!(require_authn_at(&FRESH_HWK, &ctx, NOW).is_ok());
}

#[test]
fn each_unmet_part_is_named_in_order() {
    let stale = signed_in(NOW - 301, &["hwk", "user"], Some("aal2"));
    assert_eq!(FRESH_HWK.unmet(&stale, NOW), [Unmet::MaxAge]);
    let weak = signed_in(NOW, &["pwd"], Some("aal1"));
    assert_eq!(FRESH_HWK.unmet(&weak, NOW), [Unmet::Acr, Unmet::Amr]);
    let partial = signed_in(NOW, &["hwk"], Some("aal2"));
    assert_eq!(FRESH_HWK.unmet(&partial, NOW), [Unmet::Amr]);
}

#[test]
fn a_missing_fact_fails_closed() {
    let bare = CratestackContext::builder("usr-1").build();
    assert_eq!(
        FRESH_HWK.unmet(&bare, NOW),
        [Unmet::MaxAge, Unmet::Acr, Unmet::Amr]
    );
    assert_eq!(
        FRESH_HWK.unmet(&CratestackContext::anonymous(), NOW),
        [Unmet::MaxAge, Unmet::Acr, Unmet::Amr]
    );
}

#[test]
fn an_auth_time_in_the_future_reads_as_fresh() {
    let ahead = AuthnRequirement {
        max_age: Some(60),
        ..AuthnRequirement::default()
    };
    assert!(ahead.unmet(&signed_in(NOW + 30, &[], None), NOW).is_empty());
}

#[test]
fn no_requirement_is_always_met() {
    assert!(require_authn(None, &CratestackContext::anonymous()).is_ok());
    let ctx = signed_in(0, &[], None);
    assert!(
        require_authn(Some(&FRESH_HWK), &ctx).is_err(),
        "a 1970 auth_time is stale"
    );
}

#[test]
fn the_refusal_is_rfc_9470s() {
    let ctx = signed_in(NOW - 900, &["pwd"], None);
    let Err(CratestackError::AuthRefused(refusal)) = require_authn_at(&FRESH_HWK, &ctx, NOW) else {
        panic!("a step-up refusal");
    };
    assert_eq!(refusal.message(), STEP_UP_MESSAGE);
    assert_eq!(
        refusal.members().get("error"),
        Some(&Value::String(STEP_UP_ERROR.to_owned()))
    );
    let Some(Value::Map(details)) = refusal.details() else {
        panic!("details");
    };
    assert_eq!(details.get("max_age"), Some(&Value::Int(300)));
    let list = |values: &[&str]| {
        Value::List(
            values
                .iter()
                .map(|v| Value::String((*v).to_owned()))
                .collect(),
        )
    };
    assert_eq!(details.get("acr_values"), Some(&list(&["aal2", "aal3"])));
    assert_eq!(details.get("amr"), Some(&list(&["hwk", "user"])));
    assert_eq!(
        details.get("unmet"),
        Some(&list(&["max_age", "acr", "amr"]))
    );
    assert_eq!(
        refusal.challenges(),
        [
            r#"Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300", acr_values="aal2 aal3""#
        ]
    );
}

#[test]
fn only_what_is_required_reaches_the_refusal() {
    let amr_only = AuthnRequirement {
        amr: &["hwk"],
        ..AuthnRequirement::default()
    };
    let Err(CratestackError::AuthRefused(refusal)) =
        require_authn_at(&amr_only, &CratestackContext::anonymous(), NOW)
    else {
        panic!("a step-up refusal");
    };
    let Some(Value::Map(details)) = refusal.details() else {
        panic!("details");
    };
    assert_eq!(
        details.keys().map(String::as_str).collect::<Vec<_>>(),
        ["amr", "unmet"]
    );
    assert_eq!(
        refusal.challenges(),
        [
            r#"Bearer error="insufficient_user_authentication", error_description="step-up authentication required""#
        ]
    );
}
