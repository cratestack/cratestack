//! A provider for the `@authn` tests (cratestack#1244): it builds the caller
//! from request headers, so one router covers every case.
//!
//! `x-sub` signs in; `x-auth-age` is how many seconds ago the user
//! authenticated; `x-amr`, `x-acr` and `x-roles` (space-separated) fill the
//! principal contract's claims.

#![allow(dead_code)]

use cratestack::{CratestackContext, CratestackError};

#[derive(Clone)]
pub struct HeaderProvider;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

impl cratestack::AuthProvider for HeaderProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        let header = |name: &str| request.headers.get(name).and_then(|v| v.to_str().ok());
        let Some(sub) = header("x-sub") else {
            return Ok(CratestackContext::anonymous());
        };
        let words = |name: &str| header(name).unwrap_or_default().split_whitespace();
        let mut builder = CratestackContext::builder(sub)
            .roles(words("x-roles"))
            .amr(words("x-amr"));
        if let Some(age) = header("x-auth-age").and_then(|age| age.parse::<i64>().ok()) {
            builder = builder.auth_time(now() - age);
        }
        if let Some(acr) = header("x-acr") {
            builder = builder.acr(acr);
        }
        Ok(builder.build())
    }
}

/// A caller who meets `pay`'s requirement.
pub const STRONG: &[(&str, &str)] = &[
    ("x-sub", "usr-1"),
    ("x-auth-age", "10"),
    ("x-amr", "hwk user"),
    ("x-acr", "aal2"),
];

/// The same caller, authenticated twenty minutes ago with a password.
pub const STALE_WEAK: &[(&str, &str)] = &[
    ("x-sub", "usr-1"),
    ("x-auth-age", "1200"),
    ("x-amr", "pwd"),
    ("x-acr", "aal1"),
];

/// The step-up `details` for `pay` when every part is unmet.
pub fn pay_details_all_unmet() -> serde_json::Value {
    serde_json::json!({
        "max_age": 300,
        "acr_values": ["aal2", "aal3"],
        "amr": ["hwk"],
        "unmet": ["max_age", "acr", "amr"],
    })
}

pub const PAY_CHALLENGE: &str = r#"Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300", acr_values="aal2 aal3""#;
