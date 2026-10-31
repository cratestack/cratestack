//! The step-up refusal (RFC 9470), as an `AuthRefusal` every transport
//! renders the same way.

use std::collections::BTreeMap;

use super::{AuthnRequirement, Unmet};
use crate::error::AuthRefusal;
use crate::value::Value;

/// The envelope member `error`'s value, and the challenge's `error`
/// parameter: RFC 9470's code.
pub const STEP_UP_ERROR: &str = "insufficient_user_authentication";

/// The refusal's message, and the challenge's `error_description`.
pub const STEP_UP_MESSAGE: &str = "step-up authentication required";

fn strings(values: &[&str]) -> Value {
    Value::List(
        values
            .iter()
            .map(|value| Value::String((*value).to_owned()))
            .collect(),
    )
}

pub(super) fn step_up(requirement: &AuthnRequirement, unmet: &[Unmet]) -> AuthRefusal {
    let mut details = BTreeMap::new();
    let mut challenge =
        format!(r#"Bearer error="{STEP_UP_ERROR}", error_description="{STEP_UP_MESSAGE}""#);
    if let Some(max_age) = requirement.max_age {
        details.insert("max_age".to_owned(), Value::Int(max_age));
        challenge.push_str(&format!(r#", max_age="{max_age}""#));
    }
    if !requirement.acr.is_empty() {
        details.insert("acr_values".to_owned(), strings(requirement.acr));
        challenge.push_str(&format!(r#", acr_values="{}""#, requirement.acr.join(" ")));
    }
    if !requirement.amr.is_empty() {
        details.insert("amr".to_owned(), strings(requirement.amr));
    }
    let unmet: Vec<&str> = unmet.iter().map(|part| part.as_str()).collect();
    details.insert("unmet".to_owned(), strings(&unmet));
    AuthRefusal::unauthenticated(STEP_UP_MESSAGE)
        .with_member("error", Value::String(STEP_UP_ERROR.to_owned()))
        .with_details(Value::Map(details))
        .with_challenge(challenge)
}
