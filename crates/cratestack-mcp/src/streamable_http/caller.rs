//! Who a tool call runs as, per transport.
//!
//! stdio has one caller for the whole process, stated by the application
//! (ADR 0002 Q1). Streamable HTTP has one per request, built by the
//! application's `AuthProvider` in [`super::guard`] and carried to the
//! handler through the request's extensions: `rmcp`'s HTTP service moves
//! the request's `http::request::Parts`, extensions included, into the
//! handler's [`RequestContext::extensions`] (`serve_negotiated_request_directly`
//! in `rmcp` 3.4.1's `streamable_http_server/tower.rs`).

use std::borrow::Cow;

use cratestack_core::CratestackContext;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};

pub(crate) enum Caller {
    /// stdio: the one context the application passed to `StdioServer::new`.
    Fixed(CratestackContext),
    /// Streamable HTTP: the [`AuthenticatedCaller`] the guard put on this
    /// request.
    PerRequest,
}

/// The context the guard's `AuthProvider` call produced for one request.
///
/// Crate-private on purpose. `http::Extensions` is keyed by type, so any
/// layer in the application's stack could insert a plain
/// `CratestackContext` into a request. Only this crate can name this type,
/// so only the guard, which ran the provider, can put a value where
/// [`Caller::resolve`] looks. A layer between the guard and `rmcp` cannot
/// swap in an identity of its own.
#[derive(Clone)]
pub(crate) struct AuthenticatedCaller(pub(crate) CratestackContext);

/// What the guard hands `rmcp` after authenticating: the request without
/// its `Authorization` header, carrying the caller instead. Removing the
/// header here, rather than trusting that nothing downstream logs or
/// forwards it, means `rmcp`, the handler and anything reading the
/// `Parts` it stores never hold the token at all.
pub(crate) fn hand_over(parts: &mut http::request::Parts, caller: CratestackContext) {
    parts.headers.remove(http::header::AUTHORIZATION);
    parts.extensions.insert(AuthenticatedCaller(caller));
}

impl Caller {
    pub(crate) fn resolve(
        &self,
        request: &RequestContext<RoleServer>,
    ) -> Result<Cow<'_, CratestackContext>, ErrorData> {
        match self {
            Self::Fixed(context) => Ok(Cow::Borrowed(context)),
            Self::PerRequest => request
                .extensions
                .get::<http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<AuthenticatedCaller>())
                .map(|caller| Cow::Owned(caller.0.clone()))
                // Fails closed. The guard always runs first, so this means
                // the handler was reached another way; running the call
                // anonymously would hide that instead of refusing it.
                .ok_or_else(|| {
                    tracing::error!(
                        target: "cratestack",
                        cratestack_operation = "mcp_tool_call",
                        "mcp: an HTTP request reached the handler without an authenticated caller",
                    );
                    ErrorData::internal_error("no authenticated caller for this request", None)
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::{CratestackContext, Value};

    use super::{AuthenticatedCaller, hand_over};

    #[test]
    fn the_token_is_removed_and_the_caller_attached() {
        let request = http::Request::builder()
            .header("authorization", "Bearer secret-token")
            .header("x-other", "kept")
            .body(())
            .unwrap();
        let (mut parts, ()) = request.into_parts();
        let caller =
            CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))]);

        hand_over(&mut parts, caller.clone());

        assert!(parts.headers.get("authorization").is_none());
        assert_eq!(parts.headers["x-other"], "kept");
        let attached = parts.extensions.get::<AuthenticatedCaller>().unwrap();
        assert_eq!(attached.0, caller);
    }
}
