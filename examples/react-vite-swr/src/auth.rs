//! Header-based auth, same convention as every other example in this repo
//! (`x-auth-id` -> `auth().id`). A real deployment verifies a session cookie
//! or a token here instead.
//!
//! For `clearDoneTasks`'s `@authn` (cratestack#1244) the provider also fills
//! the principal contract's `auth_time` and `amr` (ADR 0021 §2, ADR 0022)
//! from two more stand-in headers: `x-auth-time`, when the user last
//! authenticated (seconds since the epoch), and `x-auth-amr`, how
//! (space-separated RFC 8176 methods). The two apps send what their demo
//! session holds, and their step-up screen updates it. Trusting a header for
//! these facts is exactly what a real provider must not do: it reads them
//! from a verified credential. `examples/step-up-procedures` shows that with
//! CrateStack's in-house provider, which copies them from the id token.

use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};

/// Reads the caller from `x-auth-id`, `x-auth-time` and `x-auth-amr`.
#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        let header = |name: &str| request.headers.get(name).and_then(|v| v.to_str().ok());
        let Some(id) = header("x-auth-id").and_then(|raw| raw.parse::<i64>().ok()) else {
            return Ok(CratestackContext::anonymous());
        };
        let mut builder = CratestackContext::builder(id.to_string())
            .claim("id", Value::Int(id))
            .amr(header("x-auth-amr").unwrap_or_default().split_whitespace());
        if let Some(auth_time) = header("x-auth-time").and_then(|raw| raw.parse::<i64>().ok()) {
            builder = builder.auth_time(auth_time);
        }
        Ok(builder.build())
    }
}
