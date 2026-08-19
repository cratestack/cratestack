use std::convert::Infallible;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::trusted_proxy::TrustedProxyConfig;

/// The trusted-proxy configuration (if an `Extension<TrustedProxyConfig>`
/// was applied to the router), the verified socket peer (if the router
/// is served via `into_make_service_with_connect_info`), and a clone of
/// the request's full `http::Extensions` map, bundled into a single axum
/// extractor so every generated dispatch fn threads one new parameter
/// instead of several (#415).
///
/// The `extensions` field (added for the `AuthProvider::authenticate`
/// extensions plumbing, request_context — see `cratestack_core::
/// RequestContext::extensions`'s doc) is threaded through exactly this
/// struct rather than as a brand-new parameter: `ClientIpContext` is
/// already the one extractor every REST/RPC dispatch fn in the
/// generated code accepts, so reusing it means every transport picks the
/// new field up for free instead of needing its own separate threading
/// (and its own separate chance to be forgotten).
///
/// **Cost, and why it can't be avoided by borrowing instead of cloning:**
/// `extensions.clone()` runs unconditionally on every request, on every
/// transport, whether or not the installed `AuthProvider` ever reads
/// `RequestContext::extensions`. This can't be sidestepped by threading a
/// `&'a http::Extensions` instead: `FromRequestParts::from_request_parts`
/// returns an owned `Self` with no lifetime tied to its `&mut Parts`
/// argument (axum-core 0.5.6's trait signature — `Self: Sized`, no
/// borrow), and by the time a generated dispatch fn runs, the `Parts`
/// this extractor was called from no longer exists as a distinct value
/// for anything to borrow from — axum's tuple-extractor machinery
/// recombines it with the body for the next extractor in the parameter
/// list. Getting a genuine `&'a Extensions` would require every generated
/// handler to take one `axum::extract::Request` parameter and hand-roll
/// every other extraction (headers, path, query, body) that today comes
/// free from typed extractors — a rewrite of the entire handler-codegen
/// surface, not a local fix here. This is exactly the same constraint
/// `HeaderMap`'s own axum-core extractor is already under (`Ok(parts.
/// headers.clone())` — `axum-core-0.5.6/src/extract/request_parts.rs`),
/// which every generated dispatch fn already accepts unconditionally, so
/// this field's cost is the same class the framework already pays, not a
/// new one. Benchmarked (`tests_extensions_clone_cost.rs`, run via
/// `cargo test -p cratestack-axum -- --ignored --nocapture
/// extensions_clone_cost`) against a realistic served-router extensions
/// map (`ConnectInfo<SocketAddr>` + a 3-entry-allowlist
/// `TrustedProxyConfig`): **roughly 30-150ns/clone in a `--release` build**
/// (200,000-iteration loop, several runs on a loaded dev machine), the
/// same order of magnitude as — and never measured meaningfully above —
/// the `HeaderMap` clone (also ~30-150ns across the same runs) every
/// generated dispatch fn already pays unconditionally for a representative
/// 4-header set. Both are noise next to a real request's network/DB round
/// trip (microseconds-to-milliseconds). Debug builds measure ~265ns for
/// both (same relationship, uniformly slower), so this isn't a
/// debug-vs-release artifact. Most axum-ecosystem extensions (`ConnectInfo`, `MatchedPath`,
/// a `tracing::Span`) are `Copy` or `Arc`-backed and cheap to clone,
/// but `http::Extensions::clone()` is a deep clone of the typemap: a
/// consumer who inserts a large non-`Arc`-backed value into extensions
/// (a big `Vec`/`String`/owned buffer, say) now pays that clone's real
/// cost on every single request, not just when read — size your own
/// extension values accordingly, or wrap them in `Arc` before inserting.
///
/// A hand-written `FromRequestParts` impl rather than `Option<Extension<T>>`/
/// `Option<ConnectInfo<T>>` extractor parameters: axum 0.8 only extends its
/// blanket `Option<T>: FromRequestParts` impl to types implementing the
/// separate `OptionalFromRequestParts` trait, which neither `Extension<T>`
/// nor `ConnectInfo<T>` implements — so those two, wrapped in `Option`,
/// are not valid extractor parameter types on this axum version. Reading
/// `Parts::extensions` directly (via the infallible `Extensions` extractor
/// axum-core itself provides) sidesteps that entirely and never fails.
#[derive(Clone, Debug, Default)]
pub struct ClientIpContext {
    pub trusted_proxy: Option<TrustedProxyConfig>,
    pub peer: Option<SocketAddr>,
    pub extensions: http::Extensions,
}

impl ClientIpContext {
    /// Build directly from a raw `http::Extensions` map — the shared
    /// construction path used both by non-axum test harnesses that build
    /// requests by hand and by the `FromRequestParts` impl below, which
    /// delegates here rather than duplicating the field-by-field
    /// extraction logic.
    pub fn from_extensions(extensions: &http::Extensions) -> Self {
        Self {
            trusted_proxy: extensions.get::<TrustedProxyConfig>().cloned(),
            peer: extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| *addr),
            extensions: extensions.clone(),
        }
    }
}

impl<S> FromRequestParts<S> for ClientIpContext
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_extensions(&parts.extensions))
    }
}
