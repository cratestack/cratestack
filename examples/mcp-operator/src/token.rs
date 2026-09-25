//! The example audience-checking `AuthProvider` ADR 0002 Q5 asks for.
//!
//! **Example code, not a published API.** CrateStack v1 ships no generic
//! OAuth access-token provider: an application brings its own, verifying
//! its authorization server's JWTs against that server's JWKS. This one
//! stands in for that with a compact `<claims>.<signature>` token,
//! base64url JSON claims under HMAC-SHA256, so the example needs no
//! authorization server. What carries over to a real provider is the order
//! of checks: signature, issuer, **audience**, expiry and not-before, and
//! only then a context built from the claims.
//!
//! What does **not** carry over: HMAC is symmetric, so whoever can verify
//! these tokens can also mint them. A real provider verifies with the
//! authorization server's public keys and pins the algorithm itself, never
//! trusting a token header's `alg` (this format has no header to trust).
//!
//! The audience check is the one MCP insists on. A token minted for another
//! resource must not open this one, or any service a user signed in to could
//! replay the user's token here. The same verifier guards both transports:
//! over HTTP the audience is the endpoint's URL, over stdio it is
//! [`STDIO_AUDIENCE`], so a token for one never opens the other.
//!
//! Only `id` and `role` reach the context, named one by one. Copying every
//! claim across would let whoever mints tokens set any `auth()` field a
//! policy reads.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cratestack::serde_json::{Map, Value as Json, json};
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use hmac::{Hmac, KeyInit, Mac};

/// The issuer every example token names, and the authorization server the
/// RFC 9728 metadata document lists. Fictional: nothing is served there.
pub const ISSUER: &str = "https://auth.example.test";

/// The audience a stdio token must carry: the schema's resource authority.
pub const STDIO_AUDIENCE: &str = "cratestack://blog";

/// Shorter HMAC keys are refused, so a demo cannot run on `"secret"`.
pub const MIN_KEY_BYTES: usize = 32;

type HmacSha256 = Hmac<sha2::Sha256>;

/// Verifies example tokens for one audience.
#[derive(Clone)]
pub struct TokenVerifier {
    key: Arc<[u8]>,
    audience: String,
}

impl TokenVerifier {
    pub fn new(key: &[u8], audience: &str) -> Result<Self, String> {
        if key.len() < MIN_KEY_BYTES {
            return Err(format!(
                "the signing key must be at least {MIN_KEY_BYTES} bytes"
            ));
        }
        Ok(Self {
            key: key.into(),
            audience: audience.to_owned(),
        })
    }

    fn mac(&self) -> HmacSha256 {
        <HmacSha256 as KeyInit>::new_from_slice(&self.key).expect("HMAC takes any key length")
    }

    /// Signature (`verify_slice` compares in constant time), issuer,
    /// audience, expiry, not-before, then the context.
    pub fn verify(&self, token: &str) -> Result<CratestackContext, CratestackError> {
        let (payload, signature) = token.split_once('.').ok_or_else(|| refuse("malformed"))?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| refuse("malformed"))?;
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| refuse("bad signature"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| refuse("malformed"))?;
        let Ok(Json::Object(claims)) = cratestack::serde_json::from_slice(&bytes) else {
            return Err(refuse("malformed claims"));
        };
        if claims.get("iss").and_then(Json::as_str) != Some(ISSUER) {
            return Err(refuse("unknown issuer"));
        }
        // Exact equality, never a prefix. A JWT `aud` may be a list, which a
        // real provider accepts when it contains this resource exactly; this
        // format only issues a string, so a list is refused.
        if claims.get("aud").and_then(Json::as_str) != Some(self.audience.as_str()) {
            return Err(refuse("token audience is not this resource"));
        }
        let now = now();
        if claims
            .get("exp")
            .and_then(Json::as_i64)
            .is_none_or(|exp| exp <= now)
        {
            return Err(refuse("expired"));
        }
        // Optional, as in a JWT, but honoured when present.
        if let Some(nbf) = claims.get("nbf")
            && nbf.as_i64().is_none_or(|nbf| nbf > now)
        {
            return Err(refuse("not yet valid"));
        }
        context(&claims)
    }
}

fn context(claims: &Map<String, Json>) -> Result<CratestackContext, CratestackError> {
    let field = |name: &str| {
        claims
            .get(name)
            .and_then(Json::as_str)
            .map(|value| (name.to_owned(), Value::String(value.to_owned())))
            .ok_or_else(|| refuse("a token must carry `id` and `role`"))
    };
    Ok(CratestackContext::authenticated([
        field("id")?,
        field("role")?,
    ]))
}

fn refuse(reason: &str) -> CratestackError {
    CratestackError::Unauthorized(reason.to_owned())
}

fn now() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    i64::try_from(elapsed.as_secs()).expect("seconds fit in i64")
}

/// A token for `audience`, valid for `ttl_secs`. What an authorization
/// server would issue; here the `mint-token` subcommand prints one.
pub fn mint(key: &[u8], audience: &str, id: &str, role: &str, ttl_secs: i64) -> String {
    let claims = json!({
        "iss": ISSUER, "aud": audience, "exp": now().saturating_add(ttl_secs), "id": id, "role": role,
    });
    mint_claims(key, &claims)
}

/// A token over exactly `claims`, for the refusal tests.
pub fn mint_claims(key: &[u8], claims: &Json) -> String {
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{payload}.{signature}")
}

/// The HTTP side: a bearer token from `Authorization`, checked by a
/// [`TokenVerifier`] whose audience is the MCP endpoint's URL.
#[derive(Clone)]
pub struct AudienceProvider {
    verifier: TokenVerifier,
}

impl AudienceProvider {
    pub fn new(verifier: TokenVerifier) -> Self {
        Self { verifier }
    }
}

impl AuthProvider for AudienceProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        let header = request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| refuse("no token"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| refuse("not a bearer token"))?;
        self.verifier.verify(token)
    }
}
