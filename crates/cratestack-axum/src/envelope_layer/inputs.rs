//! [`BindingInputs`]: the request's half of the binding, built once.

use std::borrow::Cow;
use std::sync::Arc;

use cratestack_core::{Binding, PathParams, ResponseBinding, canonical_query};
use http::{Method, Uri};

use super::PAYLOAD_MEDIA_TYPE;
use super::bound::Bound;
use super::layer::Config;
use super::resolver::ResolvedRoute;

/// The request's half of the binding. Built once from the request and the
/// single [`ResolvedRoute`], used to open the request and, unchanged, to
/// seal its response: no plug-in is asked twice.
pub(super) struct BindingInputs {
    pub(super) config: Arc<Config>,
    pub(super) method: Method,
    pub(super) route: ResolvedRoute,
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

    pub(super) fn binding<'a>(
        &'a self,
        params: &'a [&'a str],
        response: Option<ResponseBinding>,
    ) -> Binding<'a> {
        Binding {
            audience: Cow::Borrowed(&self.config.audience),
            method: Cow::Borrowed(self.method.as_str()),
            route: Cow::Borrowed(self.route.route()),
            path_params: PathParams::Borrowed(params),
            query: self.query.as_deref().map(Cow::Borrowed),
            schema_sha: self.config.schema_sha,
            payload_media_type: Cow::Borrowed(PAYLOAD_MEDIA_TYPE),
            bound_headers: self.bound.borrowed(),
            response,
        }
    }
}
