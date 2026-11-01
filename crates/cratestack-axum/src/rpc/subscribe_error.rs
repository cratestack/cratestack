//! Errors from `GET /rpc/subscribe/{op_id}` before its stream starts:
//! always a JSON [`RpcErrorBody`] with the error's own status, whatever the
//! RPC codec.
//!
//! The SSE binding is JSON throughout (`super::sse`'s module doc), and its
//! clients send `Accept: text/event-stream`, which names no RPC codec: run
//! through the unary encoder's content negotiation, every refusal came back
//! as `406` instead of the `401`/`403` an `EventSource` client must act on.
//! A refusal's `WWW-Authenticate` challenges are kept (cratestack#1208).

use axum::http::{HeaderValue, header};
use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcErrorBody;

/// The subscribe endpoint's error response.
pub fn encode_subscribe_error(error: &CratestackError) -> Response {
    let body = serde_json::to_vec(&RpcErrorBody::from_cratestack(error))
        .unwrap_or_else(|_| br#"{"code":"internal","message":"encode failure"}"#.to_vec());
    let mut response = Response::new(axum::body::Body::from(body));
    *response.status_mut() = error.status_code();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    crate::auth_challenge::attach(response, error.auth_challenges())
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::{StatusCode, header};
    use cratestack_core::CratestackError;

    use super::encode_subscribe_error;

    #[tokio::test]
    async fn a_refusal_keeps_its_status_and_is_json() {
        let response = encode_subscribe_error(&CratestackError::Unauthorized("sign in".to_owned()));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "unauthenticated");
    }
}
