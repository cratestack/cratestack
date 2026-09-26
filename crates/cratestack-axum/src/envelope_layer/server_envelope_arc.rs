//! [`ServerEnvelope`] for `Arc<T>` (API-review nit): kept apart from the
//! trait, whose documentation alone fills its file.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use super::opened::{OpenedRequest, SealContext, Sealed};
use super::server_envelope::ServerEnvelope;

/// An envelope shared with the rest of the application (or built once and
/// handed to several layers) is still an envelope.
#[async_trait]
impl<T: ServerEnvelope + ?Sized> ServerEnvelope for Arc<T> {
    fn media_type(&self) -> &'static str {
        (**self).media_type()
    }

    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        (**self).is_envelope_content_type(content_type)
    }

    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        (**self).open_request(body, bind).await
    }

    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        context: &SealContext,
    ) -> Result<Sealed, CratestackError> {
        (**self).seal_response(payload, bind, context).await
    }
}
