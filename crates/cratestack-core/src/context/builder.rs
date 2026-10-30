//! [`CratestackContextBuilder`]: build a provider's [`CratestackContext`]
//! from the principal contract's names (ADR 0021 §2) without knowing the
//! facet layout.
//!
//! `build()` fills `auth` and `principal` exactly as
//! [`CratestackContext::from_principal`] does for the same claims (a `tenant`,
//! `actor` or `session` map becomes its facet), so `auth().<path>` and every
//! existing reader see the result. It sets no private fact: the system flag
//! and the verified signer stay out of a provider's reach.

use std::collections::BTreeMap;

use crate::value::Value;

use super::contract::KeyRole;
use super::{CratestackAuthIdentity, CratestackContext, PrincipalContext};

/// Builds an authenticated [`CratestackContext`]; start one with
/// [`CratestackContext::builder`]. A later call for the same claim
/// replaces the earlier value, except [`Self::tenant`] and the `cnf`
/// setters, which merge into the existing map.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "a builder does nothing until `build()` is called"]
pub struct CratestackContextBuilder {
    claims: BTreeMap<String, Value>,
}

impl CratestackContext {
    /// Start a context for the subject `sub` (the `sub` claim).
    pub fn builder(sub: impl Into<String>) -> CratestackContextBuilder {
        CratestackContextBuilder {
            claims: BTreeMap::from([("sub".to_owned(), Value::String(sub.into()))]),
        }
    }
}

impl CratestackContextBuilder {
    /// The `roles` claim, a list of strings.
    pub fn roles<I, S>(self, roles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.claim("roles", strings(roles))
    }

    /// `tenant.id`, in the `tenant` facet. Other members of a `tenant` map
    /// set through [`Self::claim`] are kept.
    pub fn tenant(self, id: impl Into<String>) -> Self {
        self.member("tenant", "id", Value::String(id.into()))
    }

    /// The `auth_time` claim, in seconds since the epoch.
    pub fn auth_time(self, seconds: i64) -> Self {
        self.claim("auth_time", Value::Int(seconds))
    }

    /// The `amr` claim (RFC 8176 values such as `hwk`, `user`, `pwd`).
    pub fn amr<I, S>(self, methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.claim("amr", strings(methods))
    }

    /// The `acr` claim.
    pub fn acr(self, class: impl Into<String>) -> Self {
        self.claim("acr", Value::String(class.into()))
    }

    /// `cnf.jkt`: the JWK SHA-256 thumbprint (RFC 7638), base64url.
    pub fn cnf_jkt(self, thumbprint: impl Into<String>) -> Self {
        self.cnf("jkt", Value::String(thumbprint.into()))
    }

    /// Any other `cnf` member, by its RFC 7800 (or later registry) name.
    pub fn cnf(self, member: impl Into<String>, value: Value) -> Self {
        self.member("cnf", member, value)
    }

    /// The `key_role` claim.
    pub fn key_role(self, role: KeyRole) -> Self {
        self.claim("key_role", Value::String(role.as_str().to_owned()))
    }

    /// The `exp` claim, in seconds since the epoch.
    pub fn expires_at(self, seconds: i64) -> Self {
        self.claim("exp", Value::Int(seconds))
    }

    /// A free-form claim. A reserved name set here must have the contract's
    /// type, or its typed reader reads it as absent.
    pub fn claim(mut self, name: impl Into<String>, value: Value) -> Self {
        self.claims.insert(name.into(), value);
        self
    }

    /// The context: authenticated, never a system context, with no
    /// verified signer.
    pub fn build(self) -> CratestackContext {
        let auth = CratestackAuthIdentity {
            fields: self.claims,
        };
        CratestackContext::with_principal(PrincipalContext::from_auth_identity(&auth))
    }

    fn member(mut self, map: &str, member: impl Into<String>, value: Value) -> Self {
        let entry = self
            .claims
            .entry(map.to_owned())
            .or_insert_with(|| Value::Map(BTreeMap::new()));
        if !matches!(entry, Value::Map(_)) {
            *entry = Value::Map(BTreeMap::new());
        }
        if let Value::Map(members) = entry {
            members.insert(member.into(), value);
        }
        self
    }
}

fn strings<I, S>(values: I) -> Value
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Value::List(
        values
            .into_iter()
            .map(|value| Value::String(value.into()))
            .collect(),
    )
}
