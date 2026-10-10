//! The payload types a route declares (cratestack#1168).

use super::resolver::ResolvedRoute;

/// The request and response payload types a route declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PayloadTypes {
    pub(super) request: &'static [&'static str],
    pub(super) response: &'static [&'static str],
}

impl ResolvedRoute {
    /// The payload types this route accepts and answers in, narrowing what
    /// the layer allows ([`EnvelopeLayerBuilder::payload_media_types`]) for
    /// this route alone: an op's allowed set is the layer's set intersected
    /// with this one. The built-in resolvers fill it from the schema's
    /// `capabilities.request_types` / `response_types`; a custom resolver
    /// that sets none does not narrow.
    ///
    /// ```
    /// use cratestack_axum::envelope_layer::ResolvedRoute;
    ///
    /// let route = ResolvedRoute::new("/v1/charges", vec![]).with_payload_types(
    ///     &["application/x-www-form-urlencoded"],
    ///     &["application/json"],
    /// );
    /// assert_eq!(
    ///     route.payload_request_types(),
    ///     Some(&["application/x-www-form-urlencoded"][..])
    /// );
    /// assert_eq!(route.payload_response_types(), Some(&["application/json"][..]));
    /// assert_eq!(ResolvedRoute::new("/v1/charges", vec![]).payload_request_types(), None);
    /// ```
    ///
    /// [`EnvelopeLayerBuilder::payload_media_types`]: super::EnvelopeLayerBuilder::payload_media_types
    #[must_use]
    pub fn with_payload_types(
        mut self,
        request: &'static [&'static str],
        response: &'static [&'static str],
    ) -> Self {
        self.payload_types = Some(PayloadTypes { request, response });
        self
    }

    /// The request types set by [`with_payload_types`](Self::with_payload_types).
    pub fn payload_request_types(&self) -> Option<&'static [&'static str]> {
        self.payload_types.map(|types| types.request)
    }

    /// The response types set by [`with_payload_types`](Self::with_payload_types).
    pub fn payload_response_types(&self) -> Option<&'static [&'static str]> {
        self.payload_types.map(|types| types.response)
    }

    pub(super) fn payload_types(&self) -> Option<PayloadTypes> {
        self.payload_types
    }
}
