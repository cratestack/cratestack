//! [`AuthRefusal`] — how an [`AuthProvider`](crate::AuthProvider) says
//! *why* it refused a request, in a form the client can act on
//! (cratestack#1208).
//!
//! A plain [`CratestackError::Unauthorized`](super::CratestackError) carries
//! a message and nothing else, so every refusal reaches the client as the
//! same `UNAUTHORIZED` / `unauthenticated` code. A client that must tell
//! "renew your credential and retry once" from "your key is gone, sign out"
//! cannot. An `AuthRefusal` keeps that fixed code (so existing clients keep
//! working) and adds what the provider chooses:
//!
//! - **members**: extra top-level members of the error envelope, next to
//!   `code` and `message` (`{"code": "UNAUTHORIZED", "message": "...",
//!   "error": "attestation_expired"}`). They can never replace `code`,
//!   `message` or `details`: [`AuthRefusal::with_member`] drops those names.
//! - **details**: the envelope's `details` member.
//! - **challenges**: `WWW-Authenticate` header values (RFC 9110 §11.6.1),
//!   for instance an RFC 6750 or RFC 9470 challenge. A value that is not
//!   printable ASCII is dropped by [`AuthRefusal::with_challenge`].
//!
//! The status is 401 ([`AuthRefusal::unauthenticated`]) or 403
//! ([`AuthRefusal::forbidden`]), never anything else: a refusal is about
//! who the caller is, so the RPC code stays `unauthenticated` or
//! `permission_denied`.

use std::collections::BTreeMap;

use crate::value::Value;

/// The envelope members an [`AuthRefusal`] can never set as an extra
/// member: they belong to the framework.
pub const RESERVED_ENVELOPE_MEMBERS: [&str; 3] = ["code", "message", "details"];

/// Whether the caller is unknown (401) or known and refused (403).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthRefusalKind {
    /// 401: the request carries no credential the provider accepts.
    Unauthenticated,
    /// 403: the provider knows the caller and refuses it.
    Forbidden,
}

/// A refusal an [`AuthProvider`](crate::AuthProvider) returns, as
/// [`CratestackError::AuthRefused`](super::CratestackError) through
/// [`AuthRefusal::into_error`]. See the module documentation.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthRefusal {
    kind: AuthRefusalKind,
    // Boxed so `CratestackError` stays as small as its other variants.
    body: Box<RefusalBody>,
}

#[derive(Debug, Clone, PartialEq)]
struct RefusalBody {
    message: String,
    members: BTreeMap<String, Value>,
    details: Option<Value>,
    challenges: Vec<String>,
}

impl AuthRefusal {
    /// A 401 refusal (`UNAUTHORIZED`, RPC `unauthenticated`).
    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(AuthRefusalKind::Unauthenticated, message.into())
    }

    /// A 403 refusal (`FORBIDDEN`, RPC `permission_denied`).
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(AuthRefusalKind::Forbidden, message.into())
    }

    fn new(kind: AuthRefusalKind, message: String) -> Self {
        Self {
            kind,
            body: Box::new(RefusalBody {
                message,
                members: BTreeMap::new(),
                details: None,
                challenges: Vec::new(),
            }),
        }
    }

    /// Adds a top-level envelope member. A name in
    /// [`RESERVED_ENVELOPE_MEMBERS`] is ignored: a refusal cannot replace
    /// the framework's `code`, `message` or `details` this way.
    #[must_use]
    pub fn with_member(mut self, name: impl Into<String>, value: Value) -> Self {
        let name = name.into();
        if !RESERVED_ENVELOPE_MEMBERS.contains(&name.as_str()) {
            self.body.members.insert(name, value);
        }
        self
    }

    /// Sets the envelope's `details` member.
    #[must_use]
    pub fn with_details(mut self, details: Value) -> Self {
        self.body.details = Some(details);
        self
    }

    /// Adds a `WWW-Authenticate` header value, e.g.
    /// `Bearer error="invalid_token"`. A value with anything but visible
    /// ASCII, spaces and tabs (a newline, a non-ASCII byte) is ignored, so
    /// a refusal can never break the response it rides on, and every client
    /// can read the header as text.
    #[must_use]
    pub fn with_challenge(mut self, challenge: impl Into<String>) -> Self {
        let challenge = challenge.into();
        let printable = challenge
            .bytes()
            .all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte));
        if printable && http::HeaderValue::from_str(&challenge).is_ok() {
            self.body.challenges.push(challenge);
        }
        self
    }

    /// The [`CratestackError`](super::CratestackError) to return from
    /// `AuthProvider::authenticate`: `Err(refusal.into_error())`.
    pub fn into_error(self) -> super::CratestackError {
        super::CratestackError::AuthRefused(self)
    }

    /// 401 or 403.
    pub const fn kind(&self) -> AuthRefusalKind {
        self.kind
    }

    /// The public message, the envelope's `message`.
    pub fn message(&self) -> &str {
        &self.body.message
    }

    /// The extra top-level envelope members.
    pub fn members(&self) -> &BTreeMap<String, Value> {
        &self.body.members
    }

    /// The envelope's `details`, if any.
    pub fn details(&self) -> Option<&Value> {
        self.body.details.as_ref()
    }

    /// The `WWW-Authenticate` header values, in the order they were added.
    pub fn challenges(&self) -> &[String] {
        &self.body.challenges
    }

    /// The message, `details` and members, moved out for the envelope.
    pub(crate) fn into_parts(self) -> (String, Option<Value>, BTreeMap<String, Value>) {
        let body = *self.body;
        (body.message, body.details, body.members)
    }
}

// Deliberately no `From<AuthRefusal> for CratestackError`: a second `From`
// impl on `CratestackError` makes every `?` whose error type was inferred
// through the reflexive `From<T> for T` ambiguous (E0283), in generated code
// and in an app's code alike. A refusal converts explicitly, with
// `into_error`.
