//! The principal contract (ADR 0021 §2, cratestack#1243): the claim names
//! the framework reserves, the type each one must have, and the typed
//! readers that answer them.
//!
//! Every reader returns `None` for an absent claim **and** for a claim of the
//! wrong type, so a reader fails closed: a `roles` that is not a list of
//! strings has no roles, a numeric `acr` has no class. Any name not listed in
//! [`RESERVED_CLAIMS`] stays free-form, read through
//! [`CratestackContext::auth_field`] as before.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::value::Value;

use super::CratestackContext;

/// The type a reserved claim must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimType {
    /// A string.
    String,
    /// A list whose every element is a string.
    StringList,
    /// An integer (seconds since the epoch for the time claims).
    Int,
    /// A map (`cnf`, with RFC 7800 member names).
    Map,
}

/// One reserved claim: its path (a dotted path for a facet member such as
/// `tenant.id`) and the type it must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservedClaim {
    pub path: &'static str,
    pub ty: ClaimType,
}

/// The reserved claim names, in the ADR's order. `id` is deliberately not
/// one of them: many schemas declare `auth User { id Int }`.
pub const RESERVED_CLAIMS: &[ReservedClaim] = &[
    claim("sub", ClaimType::String),
    claim("roles", ClaimType::StringList),
    claim("role", ClaimType::String),
    claim("tenant.id", ClaimType::String),
    claim("auth_time", ClaimType::Int),
    claim("amr", ClaimType::StringList),
    claim("acr", ClaimType::String),
    claim("cnf", ClaimType::Map),
    claim("cnf.jkt", ClaimType::String),
    claim("key_role", ClaimType::String),
    claim("exp", ClaimType::Int),
];

const fn claim(path: &'static str, ty: ClaimType) -> ReservedClaim {
    ReservedClaim { path, ty }
}

/// Which device key proved *this request* (the `key_role` claim): one that
/// never prompts, or one that prompts for user presence on every signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyRole {
    Ambient,
    UserPresent,
}

impl KeyRole {
    /// The claim's wire value: `ambient` or `user_present`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ambient => "ambient",
            Self::UserPresent => "user_present",
        }
    }

    /// The role a claim value names; `None` for any other string.
    pub fn from_claim(value: &str) -> Option<Self> {
        match value {
            "ambient" => Some(Self::Ambient),
            "user_present" => Some(Self::UserPresent),
            _ => None,
        }
    }
}

impl CratestackContext {
    /// Who the caller is, for every framework read of "who" (audit actor,
    /// MCP admission, the sync principal). Reads `sub` (a string), then
    /// `actor.id`, then `id`; for the last two an integer renders as its
    /// decimal digits. A candidate of any other type reads as absent and
    /// the next one is tried.
    pub fn subject(&self) -> Option<Cow<'_, str>> {
        if let Some(Value::String(sub)) = self.auth_field("sub") {
            return Some(Cow::Borrowed(sub.as_str()));
        }
        ["actor.id", "id"]
            .into_iter()
            .find_map(|path| match self.auth_field(path)? {
                Value::String(id) => Some(Cow::Borrowed(id.as_str())),
                Value::Int(id) => Some(Cow::Owned(id.to_string())),
                _ => None,
            })
    }

    /// The `roles` claim. `None` when absent or when any element is not a
    /// string.
    pub fn roles(&self) -> Option<Vec<&str>> {
        string_list(self.auth_field("roles")?)
    }

    /// The `auth_time` claim: when the user last actively authenticated, in
    /// seconds since the epoch.
    pub fn auth_time(&self) -> Option<i64> {
        int(self.auth_field("auth_time")?)
    }

    /// The `amr` claim: the authentication methods (RFC 8176 values).
    pub fn amr(&self) -> Option<Vec<&str>> {
        string_list(self.auth_field("amr")?)
    }

    /// The `acr` claim: the authentication context class.
    pub fn acr(&self) -> Option<&str> {
        string(self.auth_field("acr")?)
    }

    /// A member of the `cnf` (proof-of-possession confirmation) map, by its
    /// RFC 7800 name. `None` when `cnf` is absent or is not a map.
    pub fn cnf(&self, member: &str) -> Option<&Value> {
        match self.auth_field("cnf")? {
            Value::Map(members) => members.get(member),
            _ => None,
        }
    }

    /// `cnf.jkt`: the JWK SHA-256 thumbprint (RFC 7638), base64url.
    pub fn cnf_jkt(&self) -> Option<&str> {
        string(self.cnf("jkt")?)
    }

    /// The `key_role` claim; `None` for any value but `ambient` or
    /// `user_present`.
    pub fn key_role(&self) -> Option<KeyRole> {
        KeyRole::from_claim(string(self.auth_field("key_role")?)?)
    }

    /// The `exp` claim: when the authentication expires, in seconds since
    /// the epoch.
    pub fn expires_at(&self) -> Option<i64> {
        int(self.auth_field("exp")?)
    }
}

fn string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

fn string_list(value: &Value) -> Option<Vec<&str>> {
    match value {
        Value::List(items) => items.iter().map(string).collect(),
        _ => None,
    }
}
