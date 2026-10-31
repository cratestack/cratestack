//! Authenticating a session, and refusing one (design §6a.4; ADR 0021 §1,
//! cratestack-docs `internals/auth-seam-adr.md`).
//!
//! A refusal is never just a close code. The server first writes
//! `{id: 0, err: RpcErrorBody}` on the control stream — `Init` is id 0 and
//! `Ready{0}` acknowledges it, so the frame already belongs to the protocol
//! — then closes the session. The body comes from
//! `RpcErrorBody::from_cratestack`, so an `AuthRefusal`'s `details` and
//! extra members (the `error` reason a device-signature client switches on)
//! reach the client the same way they do on REST and RPC. Its
//! `WWW-Authenticate` challenges do not: a frame has no headers, and
//! ADR 0021 Q5 keeps them out of the body for now.

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack_core::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use http::{HeaderMap, HeaderName, HeaderValue};

use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame, close_code};
use crate::transport::SendHalf;

/// How long a refusal waits for the client to acknowledge its `err` frame
/// before closing anyway.
const REFUSAL_GRACE: Duration = Duration::from_secs(2);

/// What the CONNECT carried that authentication needs later.
pub(crate) struct Connect {
    pub(crate) path: String,
    pub(crate) query: Option<String>,
    pub(crate) headers: HeaderMap,
}

/// The first control frame must be `Init`.
pub(crate) fn init_headers(frame: ClientFrame) -> Option<BTreeMap<String, String>> {
    match frame.classify() {
        Ok(ClientMessage::Init(init)) => Some(init.headers),
        _ => None,
    }
}

/// Runs the app's `AuthProvider` over the CONNECT request. `Init` headers,
/// when present, stand in for the CONNECT headers: that is how a browser,
/// which cannot set headers on `new WebTransport(url)`, presents
/// credentials. A native client sends them on CONNECT and an empty `Init`.
pub(crate) async fn authenticate<A: AuthProvider>(
    auth: &A,
    connect: &Connect,
    init_headers: &BTreeMap<String, String>,
) -> Result<CratestackContext, CratestackError> {
    let from_init;
    let headers = if init_headers.is_empty() {
        &connect.headers
    } else {
        let mut headers = HeaderMap::new();
        for (name, value) in init_headers {
            let name =
                HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()).map_err(|_| {
                    CratestackError::BadRequest(format!("invalid Init header name `{name}`"))
                })?;
            let value = HeaderValue::from_str(value).map_err(|_| {
                CratestackError::BadRequest(format!("invalid Init header value for `{name}`"))
            })?;
            headers.append(name, value);
        }
        from_init = headers;
        &from_init
    };
    let extensions = http::Extensions::new();
    let request = RequestContext {
        method: "CONNECT",
        path: &connect.path,
        query: connect.query.as_deref(),
        headers,
        body: &[],
        extensions: &extensions,
    };
    auth.authenticate(&request).await.map_err(Into::into)
}

/// The close code a refusal ends the session with, from the error's HTTP
/// status: 401 → 4401 and 403 → 4403, as on REST; a malformed `Init`
/// → 4400; and a provider that could not decide (5xx: a key server down)
/// → 4500, which is **not** a verdict on the credentials — a client must
/// retry later rather than sign the user out.
pub(crate) fn refusal_close_code(error: &CratestackError) -> u32 {
    let status = error.status_code();
    match status.as_u16() {
        401 => close_code::UNAUTHENTICATED,
        403 => close_code::FORBIDDEN,
        400 => close_code::PROTOCOL,
        _ if status.is_server_error() => close_code::SERVER_ERROR,
        _ => close_code::UNAUTHENTICATED,
    }
}

/// Writes the refusal's `{id: 0, err}` frame, finishes the control stream
/// and waits — bounded — until the client has acknowledged it, so the
/// session close that follows cannot overtake the frame. Returns the close
/// code and reason for the caller to close with.
pub(crate) async fn refuse(
    control: &mut SendHalf,
    codec: WireCodec,
    error: &CratestackError,
    reason: &'static str,
) -> (u32, &'static str) {
    let code = refusal_close_code(error);
    tracing::info!(target: "cratestack", close = code, error = %error, "WebTransport session refused");
    if let Ok(frame) = codec.encode(&ServerFrame::err(0, error))
        && control.write_all(&frame).await.is_ok()
    {
        control.finish();
        let _ = tokio::time::timeout(REFUSAL_GRACE, control.stopped()).await;
    }
    (code, reason)
}

#[cfg(test)]
mod tests {
    use cratestack_core::AuthRefusal;

    use super::*;

    #[test]
    fn close_codes_follow_the_refusal_status() {
        let code = |e: CratestackError| refusal_close_code(&e);
        assert_eq!(
            code(CratestackError::Unauthorized("x".into())),
            close_code::UNAUTHENTICATED
        );
        assert_eq!(
            code(AuthRefusal::unauthenticated("x").into_error()),
            close_code::UNAUTHENTICATED
        );
        assert_eq!(
            code(CratestackError::Forbidden("x".into())),
            close_code::FORBIDDEN
        );
        assert_eq!(
            code(AuthRefusal::forbidden("x").into_error()),
            close_code::FORBIDDEN
        );
        assert_eq!(
            code(CratestackError::BadRequest("x".into())),
            close_code::PROTOCOL
        );
        assert_eq!(
            code(CratestackError::Unavailable("x".into())),
            close_code::SERVER_ERROR
        );
        assert_eq!(
            code(CratestackError::Internal("x".into())),
            close_code::SERVER_ERROR
        );
    }
}
