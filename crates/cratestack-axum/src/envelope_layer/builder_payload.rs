//! [`EnvelopeLayerBuilder::payload_media_types`] (cratestack#1168).

use cratestack_core::{CratestackError, DEFAULT_PAYLOAD_MEDIA_TYPE, is_sealable_payload_type};

use super::builder::EnvelopeLayerBuilder;

impl EnvelopeLayerBuilder {
    /// The payload media types the layer allows inside the seal: `request`
    /// for what a client seals, `response` for what the layer seals back.
    /// **Default: `application/cbor` for both**, so a layer that never calls
    /// this changes nothing for anyone: a request that names no type is
    /// bound and opened with the bytes 0.15.3 used (AAD and body), and
    /// answered by a response whose AAD and body are 0.15.3's too. The one
    /// difference a 0.15.3 client can see is a new response header,
    /// `Cratestack-Payload-Type`, which it ignores.
    ///
    /// A client names the type of its request in the unbound
    /// `Cratestack-Payload-Type` header and the response types it reads, in
    /// order, in `Cratestack-Payload-Accept` (the same selector pattern as
    /// `Cratestack-Contract`: they select among what is allowed and never
    /// widen it). Both are authenticated by the binding, not by the headers:
    /// a request binding names the request payload's type, a response
    /// binding names the **response payload's own** type, and a response
    /// repeats it in `Cratestack-Payload-Type`. A header that lies about the
    /// request fails the signature with the coarse `401`. Binding version 2
    /// and the COSE wire are unchanged.
    ///
    /// What an op allows is this set intersected with the route's declared
    /// types ([`ResolvedRoute::with_payload_types`], filled from the schema's
    /// capabilities by [`RestBindingResolver`] and [`RpcBindingResolver`]).
    /// `/rpc/batch` stays CBOR both ways. An empty route list (the generated
    /// reads and deletes, which take no payload) constrains nothing, and a
    /// `GET`, `HEAD` or `DELETE` is not checked for the type its **empty**
    /// payload names (bound as sent, never refused): a payload that is not
    /// empty is held to this set and the route's once opened, and refused as
    /// a sealed `415`.
    ///
    /// A retry under the same `Idempotency-Key` must repeat its
    /// `Cratestack-Payload-Accept`: the idempotency layer fingerprints the
    /// request's content type and body, not the types it reads, so a
    /// response stored for the first attempt in a type the retry did not
    /// negotiate is never sealed for it (a sealed `500`, fail closed).
    ///
    /// Refusals are unsigned and made before any key is looked up or nonce
    /// spent: a selector header sent twice or malformed is a `400`, a request
    /// type outside the op's set a `415` (`payload_type_unsupported`), and a
    /// `Cratestack-Payload-Accept` with no type the op can answer in a `406`
    /// (`payload_type_not_acceptable`). A handler's success in a type the
    /// request did not negotiate is a sealed `500`; its error in one is
    /// re-encoded in the transport's error shape, in the client's first
    /// choice of CBOR or JSON, and sealed.
    ///
    /// [`build`](Self::build) refuses a type outside the grammar of
    /// `cratestack_core::parse_payload_type`, one that may never be sealed
    /// (`application/cose*`, `application/cbor-seq`, `text/event-stream`,
    /// `multipart/*`), an empty request set, and a response set without
    /// `application/cbor` or `application/json`, which the layer's own
    /// errors are sealed in.
    ///
    /// ```
    /// use cratestack_axum::envelope_layer::EnvelopeLayer;
    /// # fn demo(envelope: impl cratestack_axum::envelope_layer::ServerEnvelope) {
    /// // vpay's shape: forms in, JSON out, and CBOR for the generated clients.
    /// let builder = EnvelopeLayer::builder(envelope, "payments", &[]).payload_media_types(
    ///     ["application/cbor", "application/x-www-form-urlencoded"],
    ///     ["application/cbor", "application/json"],
    /// );
    /// # drop(builder);
    /// # }
    /// ```
    ///
    /// [`ResolvedRoute::with_payload_types`]: super::ResolvedRoute::with_payload_types
    /// [`RestBindingResolver`]: super::RestBindingResolver
    /// [`RpcBindingResolver`]: super::RpcBindingResolver
    #[must_use]
    pub fn payload_media_types<I, J>(mut self, request: I, response: J) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
        J: IntoIterator,
        J::Item: Into<String>,
    {
        self.payload_request = request.into_iter().map(Into::into).collect();
        self.payload_response = response.into_iter().map(Into::into).collect();
        self
    }
}

/// The checks `build()` makes of the configured payload types.
pub(super) fn validate(request: &[String], response: &[String]) -> Result<(), CratestackError> {
    let invalid = |message: String| Err(CratestackError::Validation(message));
    if request.is_empty() {
        return invalid("payload_media_types(..): the request set must not be empty".to_owned());
    }
    for (side, types) in [("request", request), ("response", response)] {
        if let Some(bad) = types.iter().find(|t| !is_sealable_payload_type(t)) {
            return invalid(format!(
                "payload_media_types(..): {bad:?} cannot be a sealed {side} payload type \
                 (a lowercase `type/subtype` without parameters or wildcards, and never an \
                 envelope, a stream or a multipart body)"
            ));
        }
    }
    // The layer's own errors, and a handler's foreign-typed ones, are sealed
    // in a type the transport's error codec writes.
    if !response
        .iter()
        .any(|t| t == DEFAULT_PAYLOAD_MEDIA_TYPE || t == "application/json")
    {
        return invalid(
            "payload_media_types(..): the response set needs application/cbor or \
             application/json, which the layer's own errors are sealed in"
                .to_owned(),
        );
    }
    Ok(())
}
