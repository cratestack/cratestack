//! Admitting a subscription session, whatever carries it
//! (`docs/design/subscriptions-websocket.md` §6.3): the first frame must be
//! `Init`, its headers stand in for the handshake's when present, and a
//! refusal's close code follows the error's status.

use std::collections::BTreeMap;

use http::{HeaderMap, HeaderName, HeaderValue};

use super::frame::{ClientFrame, ClientMessage, close_code};
use crate::{AuthProvider, CratestackContext, CratestackError, RequestContext};

/// What the handshake (WebTransport CONNECT, WebSocket upgrade) carried
/// that authentication needs later.
#[derive(Debug, Clone)]
pub struct Handshake {
    pub method: &'static str,
    pub path: String,
    pub query: Option<String>,
    pub headers: HeaderMap,
}

/// The first frame must be `Init`: its headers, else `None`.
pub fn init_headers(frame: ClientFrame) -> Option<BTreeMap<String, String>> {
    match frame.classify() {
        Ok(ClientMessage::Init(init)) => Some(init.headers),
        _ => None,
    }
}

/// Runs the app's `AuthProvider` over the handshake request. `Init` headers,
/// when present, stand in for the handshake's: that is how a browser, which
/// cannot set headers on `new WebTransport(url)` or `new WebSocket(url)`,
/// presents credentials. A native client sends them on the handshake and an
/// empty `Init`.
pub async fn authenticate_session<A: AuthProvider>(
    auth: &A,
    handshake: &Handshake,
    init_headers: &BTreeMap<String, String>,
) -> Result<CratestackContext, CratestackError> {
    let from_init;
    let headers = if init_headers.is_empty() {
        &handshake.headers
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
        method: handshake.method,
        path: &handshake.path,
        query: handshake.query.as_deref(),
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
pub fn refusal_close_code(error: &CratestackError) -> u32 {
    let status = error.status_code();
    match status.as_u16() {
        401 => close_code::UNAUTHENTICATED,
        403 => close_code::FORBIDDEN,
        400 => close_code::PROTOCOL,
        _ if status.is_server_error() => close_code::SERVER_ERROR,
        _ => close_code::UNAUTHENTICATED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthRefusal;

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
