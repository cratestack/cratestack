//! A step-up refusal, typed (cratestack#1244, the wire contract of ADR 0022).
//!
//! An operation that requires a fresher or stronger authentication than the
//! caller presented is refused with an `AuthRefusal` whose extra member
//! `error` is `insufficient_user_authentication` (RFC 9470): HTTP 401 on
//! REST, code `unauthenticated` on RPC (unary and per batch frame). Its
//! `details` name what the operation requires (`max_age`, `acr_values`,
//! `amr`) and which of them the caller failed (`unmet`).
//!
//! [`StepUpRequired`] is that refusal read from either error type; reading
//! never fails: a missing or mistyped field reads as absent or empty.

use serde_json::Value as Json;

use crate::error::ClientError;
use crate::rpc::error::{RpcClientError, RpcRemoteError};

mod retry;

pub use retry::retry_after_step_up;

/// The extra member `error` of a step-up refusal (RFC 9470).
pub const INSUFFICIENT_USER_AUTHENTICATION: &str = "insufficient_user_authentication";

/// What a step-up refusal asks for (cratestack#1244). Each requirement is
/// present only when the refused operation declares it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepUpRequired {
    /// The longest time, in seconds, since the caller last authenticated.
    pub max_age: Option<u64>,
    /// The authentication context classes the operation accepts.
    pub acr_values: Vec<String>,
    /// The authentication methods the operation requires.
    pub amr: Vec<String>,
    /// Which requirements the caller failed: a subset of `"max_age"`,
    /// `"acr"` and `"amr"`.
    pub unmet: Vec<String>,
}

impl StepUpRequired {
    /// Read a refusal from its `details` and extra members, once the
    /// transport's own test (401, or `unauthenticated`) has passed. `None`
    /// unless `extra["error"]` is [`INSUFFICIENT_USER_AUTHENTICATION`].
    fn from_refusal(details: Option<Json>, error_member: Option<&Json>) -> Option<Self> {
        if error_member.and_then(Json::as_str) != Some(INSUFFICIENT_USER_AUTHENTICATION) {
            return None;
        }
        let details = match details {
            Some(Json::Object(details)) => details,
            _ => Default::default(),
        };
        let strings = |name: &str| -> Vec<String> {
            match details.get(name) {
                Some(Json::Array(items)) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            }
        };
        Some(Self {
            max_age: details.get("max_age").and_then(Json::as_u64),
            acr_values: strings("acr_values"),
            amr: strings("amr"),
            unmet: strings("unmet"),
        })
    }
}

/// An error that may be a step-up refusal: what [`retry_after_step_up`]
/// reads. Implemented by [`ClientError`], [`RpcClientError`] and
/// [`RpcRemoteError`].
pub trait StepUpError {
    /// The refusal, when this error is one.
    fn step_up(&self) -> Option<StepUpRequired>;
}

impl ClientError {
    /// The step-up refusal this error carries: a 401 whose envelope's
    /// extra member `error` is `insufficient_user_authentication`. `None`
    /// for anything else.
    pub fn step_up(&self) -> Option<StepUpRequired> {
        let ClientError::Remote { status, error, .. } = self else {
            return None;
        };
        if status.as_u16() != 401 {
            return None;
        }
        let error = error.as_deref()?;
        let member = error.extra.get("error").map(|value| value.to_plain_json());
        let details = error.details.as_ref().map(|value| value.to_plain_json());
        StepUpRequired::from_refusal(details, member.as_ref())
    }
}

impl RpcRemoteError {
    /// The step-up refusal this answer carries: code `unauthenticated` and
    /// the extra member `error` set to `insufficient_user_authentication`.
    /// A batch frame reads the same as a unary answer.
    pub fn step_up(&self) -> Option<StepUpRequired> {
        if self.body.code != "unauthenticated" {
            return None;
        }
        StepUpRequired::from_refusal(self.body.details.clone(), self.body.extra.get("error"))
    }
}

impl RpcClientError {
    /// The step-up refusal of an [`RpcClientError::Remote`]; `None` for
    /// any other variant.
    pub fn step_up(&self) -> Option<StepUpRequired> {
        self.remote().and_then(RpcRemoteError::step_up)
    }
}

impl StepUpError for ClientError {
    fn step_up(&self) -> Option<StepUpRequired> {
        ClientError::step_up(self)
    }
}

impl StepUpError for RpcRemoteError {
    fn step_up(&self) -> Option<StepUpRequired> {
        RpcRemoteError::step_up(self)
    }
}

impl StepUpError for RpcClientError {
    fn step_up(&self) -> Option<StepUpRequired> {
        RpcClientError::step_up(self)
    }
}

#[cfg(test)]
mod tests;
