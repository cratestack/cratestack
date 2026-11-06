//! What a client carries to seal its calls (cratestack#1007).
//!
//! One field on [`CratestackClient`], so the constructors (`new`,
//! `with_http_client`, `with_middleware_client`) initialise it with
//! `Default` and none of them knows whether the `cose` feature is on. Without
//! the feature the struct is empty and `at` is a plain clone. Setting the
//! envelope and the op contracts is in `sealing_setup.rs`.

use crate::client::core::CratestackClient;
use crate::client::route::RouteRef;
use crate::codec::HttpClientCodec;

#[cfg(feature = "cose")]
use crate::client::contract::Contracts;
#[cfg(feature = "cose")]
use crate::envelope::ClientEnvelope;

/// A route detached from the call that named it.
#[cfg(feature = "cose")]
#[derive(Debug, Clone)]
pub(crate) struct OwnedRoute {
    pub(crate) template: String,
    pub(crate) params: Vec<String>,
}

#[derive(Clone, Default)]
pub(crate) struct Sealing {
    #[cfg(feature = "cose")]
    pub(crate) envelope: Option<ClientEnvelope>,
    /// The op contract digests the binding needs (`client/contract.rs`).
    #[cfg(feature = "cose")]
    pub(crate) contracts: Option<Contracts>,
    /// The route of the call in flight, set by [`CratestackClient::at`].
    #[cfg(feature = "cose")]
    pub(crate) route: Option<OwnedRoute>,
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// A copy of this client that knows which route its next call is for,
    /// so a sealed call can bind the route template (see [`RouteRef`]).
    ///
    /// Generated code calls it before every REST operation
    /// (`runtime.at(RouteRef::new("/widgets/{id}", &[&id])).get(..)`) and the
    /// RPC client before every op. It is a copy rather than a `*_at` twin of
    /// each of the twelve request methods, the same shape as
    /// [`with_idempotency`](Self::with_idempotency); it does nothing unless an
    /// envelope is set.
    #[must_use]
    pub fn at(&self, route: RouteRef<'_>) -> Self {
        #[cfg(not(feature = "cose"))]
        let _ = route;
        #[allow(unused_mut)]
        let mut client = self.clone();
        client.compact_route = client.compact.as_ref().map(|_| route.template().into());
        #[cfg(feature = "cose")]
        if client.sealing.envelope.is_some() {
            client.sealing.route = Some(OwnedRoute {
                template: route.template().to_owned(),
                params: route.params().iter().map(|p| (*p).to_owned()).collect(),
            });
        }
        client
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// This client as it must send a raw bridge request to `path`
    /// ([`execute_raw_transport`](Self::execute_raw_transport)): itself when
    /// it has no envelope, otherwise scoped to the request's route.
    ///
    /// A raw request has no generated code to name its route, so only RPC
    /// paths qualify: `/rpc/{op_id}` binds the op id, `/rpc/batch` binds
    /// `batch`. A raw REST path has no template to bind, and is refused
    /// rather than sealed for a route the server would not recognise.
    pub(crate) fn scoped_for_raw_path(
        &self,
        path: &str,
    ) -> Result<std::borrow::Cow<'_, Self>, crate::error::ClientError> {
        #[cfg(feature = "cose")]
        if self.sealing.envelope.is_some() {
            let op = crate::client::raw_path::rpc_op(path).ok_or_else(|| {
                crate::error::ClientError::BadInput(format!(
                    "a sealed raw request must be an RPC one (/rpc/{{op_id}} or /rpc/batch); \
                         '{path}' has no route template to bind"
                ))
            })?;
            return Ok(std::borrow::Cow::Owned(self.at(RouteRef::rpc(op))));
        }
        let _ = path;
        Ok(std::borrow::Cow::Borrowed(self))
    }
}
