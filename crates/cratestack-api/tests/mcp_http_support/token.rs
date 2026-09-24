//! An audience-checking `AuthProvider`: the example ADR 0002 Q5 asks for,
//! kept in tests on purpose and not shipped. CrateStack v1 has no generic
//! OAuth access-token provider; an application brings its own, and this
//! shows the checks MCP requires of one.
//!
//! The token is a compact, JWT-shaped `<claims>.<signature>`: base64url JSON
//! claims, HMAC-SHA256 over them. Real deployments verify an authorization
//! server's JWTs against its JWKS. What carries over is the order of checks:
//! signature, issuer, **audience**, expiry, then a context from the claims.
//! The audience check is the one MCP insists on: a token minted for another
//! resource must not open this one (it would let any service a user signed
//! in to replay their token here).
//!
//! A twin of `cratestack-mcp`'s `tests/support/token.rs`; a test helper
//! cannot be shared across crates without publishing it.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value as Json, json};

pub const ISSUER: &str = "https://auth.example.test";
const KEY: &[u8] = b"cratestack-api test signing key; not a secret";

#[derive(Clone)]
pub struct AudienceProvider {
    audience: String,
}

impl AudienceProvider {
    /// `audience` is this server's resource identifier, the value a token
    /// meant for it carries in `aud`.
    pub fn new(audience: &str) -> Self {
        Self {
            audience: audience.to_owned(),
        }
    }
}

fn mac() -> Hmac<sha2::Sha256> {
    <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(KEY).expect("any key length works")
}

fn now() -> i64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(elapsed.as_secs()).unwrap()
}

/// A token for `audience`, valid for an hour, carrying `claims` (at least
/// `id`) as the caller's identity.
pub fn mint(audience: &str, claims: Json) -> String {
    let mut body = json!({ "iss": ISSUER, "aud": audience, "exp": now() + 3600 });
    body.as_object_mut()
        .unwrap()
        .extend(claims.as_object().expect("claims are an object").clone());
    mint_raw(&body)
}

/// A token over exactly `body`, for the expiry and issuer cases.
pub fn mint_raw(body: &Json) -> String {
    let payload = URL_SAFE_NO_PAD.encode(body.to_string());
    let mut mac = mac();
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{payload}.{signature}")
}

fn refuse(reason: &str) -> CratestackError {
    CratestackError::Unauthorized(reason.to_owned())
}

fn verify(token: &str) -> Result<serde_json::Map<String, Json>, CratestackError> {
    let (payload, signature) = token.split_once('.').ok_or_else(|| refuse("malformed"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| refuse("malformed"))?;
    let mut mac = mac();
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| refuse("bad signature"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| refuse("malformed"))?;
    match serde_json::from_slice(&bytes) {
        Ok(Json::Object(claims)) => Ok(claims),
        _ => Err(refuse("malformed claims")),
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
        let claims = verify(token)?;
        if claims.get("iss").and_then(Json::as_str) != Some(ISSUER) {
            return Err(refuse("unknown issuer"));
        }
        if claims.get("aud").and_then(Json::as_str) != Some(self.audience.as_str()) {
            return Err(refuse("token audience is not this resource"));
        }
        if claims
            .get("exp")
            .and_then(Json::as_i64)
            .is_none_or(|exp| exp <= now())
        {
            return Err(refuse("expired"));
        }
        let identity = claims
            .into_iter()
            .filter(|(name, _)| !matches!(name.as_str(), "iss" | "aud" | "exp"))
            .filter_map(|(name, value)| match value {
                Json::String(text) => Some((name, Value::String(text))),
                Json::Number(number) => number.as_i64().map(|n| (name, Value::Int(n))),
                _ => None,
            });
        Ok(CratestackContext::authenticated(identity))
    }
}
