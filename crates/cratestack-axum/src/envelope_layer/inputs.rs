//! [`BindingInputs`]: the request's half of the binding, built once.

use std::borrow::Cow;
use std::sync::Arc;

use cratestack_core::{Binding, PathParams, ResponseBinding, canonical_query};
use http::{Method, Uri};

use super::bound::Bound;
use super::layer::Config;
use super::payload::Negotiated;
use super::resolver::ResolvedRoute;

/// The request's half of the binding. Built once from the request and the
/// single [`ResolvedRoute`], used to open the request and, unchanged, to
/// seal its response: no plug-in is asked twice.
pub(super) struct BindingInputs {
    pub(super) config: Arc<Config>,
    pub(super) method: Method,
    pub(super) route: ResolvedRoute,
    /// The op-contract digest the binding carries: the one the request
    /// opened under, which its response is sealed under too.
    pub(super) contract: [u8; 32],
    /// The payload types the request negotiated: the request's own is what
    /// its binding names, the response's is chosen per response.
    pub(super) payload: Negotiated,
    query: Option<String>,
    bound: Bound,
}

impl BindingInputs {
    pub(super) fn new(
        config: Arc<Config>,
        method: Method,
        route: ResolvedRoute,
        uri: &Uri,
        bound: Bound,
        contract: [u8; 32],
        payload: Negotiated,
    ) -> Self {
        // `canonical_query` decodes the pairs, orders them by key (a
        // repeated key's values keep their order) and re-encodes them, so
        // reordering *distinct* keys binds alike, while reordering the
        // values of one key (`?tag=a&tag=b`) does not: a list's order can
        // matter to a handler. An absent query becomes `""`; the AAD
        // encodes a query-less request as `null`, and `cratestack-cose`
        // reads an empty one the same way.
        let query = Some(canonical_query(uri.query())).filter(|query| !query.is_empty());
        Self {
            config,
            method,
            route,
            contract,
            payload,
            query,
            bound,
        }
    }

    pub(super) fn params(&self) -> Vec<&str> {
        self.route
            .path_params()
            .iter()
            .map(String::as_str)
            .collect()
    }

    /// The request's binding under `contract`, for one of several candidate
    /// digests: it names the **request** payload's type.
    pub(super) fn request_binding<'a>(
        &'a self,
        contract: [u8; 32],
        params: &'a [&'a str],
    ) -> Binding<'a> {
        self.binding_of(contract, params, &self.payload.request, None)
    }

    /// The binding of a response sealed under the digest the request opened
    /// under. It names the **response** payload's own type, which need not
    /// be the request's (a form in, JSON out).
    pub(super) fn response_binding<'a>(
        &'a self,
        params: &'a [&'a str],
        response: ResponseBinding,
        payload_type: &'a str,
    ) -> Binding<'a> {
        self.binding_of(self.contract, params, payload_type, Some(response))
    }

    fn binding_of<'a>(
        &'a self,
        contract: [u8; 32],
        params: &'a [&'a str],
        payload_type: &'a str,
        response: Option<ResponseBinding>,
    ) -> Binding<'a> {
        Binding {
            audience: Cow::Borrowed(&self.config.audience),
            method: Cow::Borrowed(self.method.as_str()),
            route: Cow::Borrowed(self.route.route()),
            path_params: PathParams::Borrowed(params),
            query: self.query.as_deref().map(Cow::Borrowed),
            contract_sha: contract,
            payload_media_type: Cow::Borrowed(payload_type),
            bound_headers: self.bound.borrowed(),
            response,
        }
    }
}
