//! What a stream is held under: [`RevocationKey`], and the default way a
//! context names its keys.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::{CratestackContext, Value};

/// A name a live stream is registered under, and a revocation is addressed
/// to. Revoking a key ends every stream and socket session holding it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RevocationKey {
    /// The principal ([`CratestackContext::subject`]): every session of
    /// one user, for a suspension.
    Subject(String),
    /// One login session (the `sid` claim): a logout.
    Session(String),
    /// The RFC 9679 thumbprint of the key that signed the open
    /// ([`VerifiedSigner::thumbprint`](crate::VerifiedSigner::thumbprint)):
    /// one enrolled device.
    Signer([u8; 32]),
    /// Anything else an app keys its revocations by.
    Custom(String),
}

impl RevocationKey {
    pub(super) fn parts(&self) -> (&'static str, String) {
        match self {
            Self::Subject(value) => ("subject", value.clone()),
            Self::Session(value) => ("session", value.clone()),
            Self::Signer(thumbprint) => ("signer", URL_SAFE_NO_PAD.encode(thumbprint)),
            Self::Custom(value) => ("custom", value.clone()),
        }
    }

    pub(super) fn from_parts(kind: &str, value: &str) -> Option<Self> {
        Some(match kind {
            "subject" => Self::Subject(value.to_owned()),
            "session" => Self::Session(value.to_owned()),
            "signer" => Self::Signer(URL_SAFE_NO_PAD.decode(value).ok()?.try_into().ok()?),
            "custom" => Self::Custom(value.to_owned()),
            _ => return None,
        })
    }
}

/// Names the keys a context's streams are held under. A plain function
/// pointer, like `PrincipalKeyFn`, so a binding config stays `Clone`.
pub type RevocationKeysFn = fn(&CratestackContext) -> Vec<RevocationKey>;

/// The keys of `ctx`: its [`subject`](CratestackContext::subject), its
/// `sid` claim when that is a string (the OIDC session id; `sid` is not a
/// reserved claim, so an app that carries a session id under another name
/// supplies its own [`RevocationKeysFn`]), and the thumbprint of the key
/// that signed the open, when it was signed.
pub fn default_revocation_keys(ctx: &CratestackContext) -> Vec<RevocationKey> {
    let mut keys = Vec::new();
    if let Some(subject) = ctx.subject() {
        keys.push(RevocationKey::Subject(subject.into_owned()));
    }
    if let Some(Value::String(sid)) = ctx.auth_field("sid") {
        keys.push(RevocationKey::Session(sid.clone()));
    }
    if let Some(signer) = ctx.verified_signer() {
        keys.push(RevocationKey::Signer(*signer.thumbprint()));
    }
    keys
}
