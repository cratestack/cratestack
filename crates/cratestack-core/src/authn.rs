//! Authentication requirements (ADR 0022, cratestack#1244): how recently and
//! how strongly a caller must have authenticated for one operation, and the
//! step-up refusal when it has not.
//!
//! A schema states the requirement next to the operation's policy; the
//! generated code holds it as an [`AuthnRequirement`] and calls
//! [`require_authn`] after the operation's `@allow`/`@deny` admitted the
//! caller. The facts come from the principal contract (ADR 0021 §2), which
//! the provider fills: `auth_time` for a freshness, `amr` for methods, `acr`
//! for a class. A missing fact fails closed.
//!
//! The refusal is RFC 9470's: a 401 whose envelope carries
//! `error: "insufficient_user_authentication"`, a `details` naming what is
//! required and what was unmet, and a `WWW-Authenticate: Bearer` challenge
//! with `max_age` / `acr_values`. A client retries at most once, after the
//! user re-proved.

mod refusal;
#[cfg(test)]
mod tests;

use crate::context::CratestackContext;
use crate::error::CratestackError;

pub use refusal::{STEP_UP_ERROR, STEP_UP_MESSAGE};

/// What one operation requires of the caller's authentication. Every part is
/// optional; a requirement with none is satisfied by any caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthnRequirement {
    /// The longest time, in seconds, since the user last actively
    /// authenticated (`auth_time`).
    pub max_age: Option<i64>,
    /// Methods that must all appear in the caller's `amr` (RFC 8176 values).
    pub amr: &'static [&'static str],
    /// Acceptable authentication classes: the caller's `acr` must be one.
    pub acr: &'static [&'static str],
}

/// Which part of a requirement the caller did not meet, in the order the
/// refusal's `details.unmet` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmet {
    /// `auth_time` is missing, or older than `max_age`.
    MaxAge,
    /// The caller's `acr` is missing or not one of the acceptable classes.
    Acr,
    /// A required method is missing from the caller's `amr`.
    Amr,
}

impl Unmet {
    /// The name `details.unmet` uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaxAge => "max_age",
            Self::Acr => "acr",
            Self::Amr => "amr",
        }
    }
}

impl AuthnRequirement {
    /// What `ctx` fails to meet at `now` (seconds since the epoch). Empty
    /// when the requirement is met. An `auth_time` in the future reads as
    /// fresh: skew is the provider's concern, not the schema's.
    pub fn unmet(&self, ctx: &CratestackContext, now: i64) -> Vec<Unmet> {
        let mut unmet = Vec::new();
        if let Some(max_age) = self.max_age {
            let fresh = ctx
                .auth_time()
                .is_some_and(|auth_time| now.saturating_sub(auth_time) <= max_age);
            if !fresh {
                unmet.push(Unmet::MaxAge);
            }
        }
        if !self.acr.is_empty() && !ctx.acr().is_some_and(|acr| self.acr.contains(&acr)) {
            unmet.push(Unmet::Acr);
        }
        if !self.amr.is_empty() {
            let have = ctx.amr().unwrap_or_default();
            if !self.amr.iter().all(|method| have.contains(method)) {
                unmet.push(Unmet::Amr);
            }
        }
        unmet
    }
}

/// `Ok` when there is no requirement or `ctx` meets it now, otherwise the
/// step-up refusal. The generated code calls it with the operation's
/// `AUTHN`, `None` for an operation without one.
pub fn require_authn(
    requirement: Option<&AuthnRequirement>,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    match requirement {
        Some(requirement) => require_authn_at(requirement, ctx, now()),
        None => Ok(()),
    }
}

/// [`require_authn`] for one requirement at a given time, for tests and
/// callers with their own clock.
pub fn require_authn_at(
    requirement: &AuthnRequirement,
    ctx: &CratestackContext,
    now: i64,
) -> Result<(), CratestackError> {
    let unmet = requirement.unmet(ctx, now);
    if unmet.is_empty() {
        return Ok(());
    }
    Err(refusal::step_up(requirement, &unmet).into_error())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
