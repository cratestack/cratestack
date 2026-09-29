//! The client half of the signed transport (ADR 0006, cratestack#1007).
//!
//! [`ClientEnvelope`] is what a client holds to seal its requests and open
//! its responses: a client-role [`CoseEnvelope`] (the signing key, the
//! server keys pinned at enrolment) plus the two facts the binding needs
//! that the envelope does not know, the recipient's `audience` and the
//! values of a parameterised mount. The sealing itself lives in
//! `client::envelope_call`, next to the transport it wraps.

use std::borrow::Cow;

use cratestack_core::{Binding, BoundHeaders, PathParams};
use cratestack_cose::{CoseEnvelope, CoseMode, CoseRole};

use crate::error::ClientError;

/// The `payload_media_type` every binding names: what is inside the seal.
const PAYLOAD_MEDIA_TYPE: &str = "application/cbor";

/// A client-role COSE envelope, addressed to one service.
///
/// ```
/// use std::sync::Arc;
/// use cratestack_client_rust::ClientEnvelope;
/// use cratestack_client_rust::cose::{
///     CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver,
/// };
///
/// let secret = vec![7; 32];
/// let signer = HmacSigner::new(CoseAlg::Hmac256_64, secret.clone()).unwrap();
/// let server_keys = StaticVerifierResolver::new().with_key(signer.verify_key());
/// let envelope = CoseEnvelope::client(CoseMode::Mac0, Arc::new(signer), Arc::new(server_keys))
///     .build()
///     .unwrap();
/// let sealed = ClientEnvelope::new(envelope, "payments").unwrap();
/// assert_eq!(sealed.mode(), CoseMode::Mac0);
/// ```
#[derive(Clone, Debug)]
pub struct ClientEnvelope {
    cose: CoseEnvelope,
    audience: Cow<'static, str>,
    mount_params: Vec<String>,
}

impl ClientEnvelope {
    /// `envelope` must be a client envelope (`CoseEnvelope::client`), and
    /// `audience` the **configured** name of the service being called, never
    /// its host name (ADR 0006 §4). An empty audience binds nothing and is
    /// refused, as is a server-role envelope, which would seal responses.
    pub fn new(
        envelope: CoseEnvelope,
        audience: impl Into<Cow<'static, str>>,
    ) -> Result<Self, ClientError> {
        let audience = audience.into();
        if envelope.role() != CoseRole::Client {
            return Err(ClientError::BadInput(
                "a ClientEnvelope needs a client-role CoseEnvelope (CoseEnvelope::client)"
                    .to_owned(),
            ));
        }
        if audience.is_empty() {
            return Err(ClientError::BadInput(
                "a ClientEnvelope needs the service's audience, which is never empty".to_owned(),
            ));
        }
        Ok(Self {
            cose: envelope,
            audience,
            mount_params: Vec::new(),
        })
    }

    /// The values of a parameterised mount's parameters, in order
    /// (`Router::nest("/t/{tenant}", ..)` is called with `vec!["acme"]`).
    /// The server binds them ahead of the route's own, so that a request
    /// signed for one tenant cannot be replayed at another.
    #[must_use]
    pub fn with_mount_params(mut self, params: Vec<String>) -> Self {
        self.mount_params = params;
        self
    }

    /// Sign1 or Mac0, from the signer the envelope was built with.
    pub fn mode(&self) -> CoseMode {
        self.cose.mode()
    }

    pub(crate) fn cose(&self) -> &CoseEnvelope {
        &self.cose
    }

    /// `Content-Type` and `Accept` of every sealed call.
    pub(crate) fn media_type(&self) -> &'static str {
        self.cose.mode().media_type()
    }

    /// The request's binding: everything the server rebuilds on its side.
    pub(crate) fn binding<'a>(
        &'a self,
        method: &'a str,
        route: &'a str,
        route_params: &'a [String],
        query: Option<String>,
        schema_sha: [u8; 32],
        bound_headers: BoundHeaders<'a>,
    ) -> Binding<'a> {
        let params = self
            .mount_params
            .iter()
            .chain(route_params)
            .cloned()
            .collect();
        Binding {
            audience: Cow::Borrowed(&self.audience),
            method: Cow::Borrowed(method),
            route: Cow::Borrowed(route),
            path_params: PathParams::Owned(params),
            query: query.map(Cow::Owned),
            schema_sha,
            payload_media_type: Cow::Borrowed(PAYLOAD_MEDIA_TYPE),
            bound_headers,
            response: None,
        }
    }
}
