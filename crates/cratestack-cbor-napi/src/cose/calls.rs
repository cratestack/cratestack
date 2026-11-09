//! The calls of an [`Envelope`]: seal, the two-step seal, the selector
//! headers and opening a response.

use bytes::Bytes;
use cratestack_cose::Opened;

use super::drive::ready;
use super::envelope::{Envelope, OpenedResponse};
use super::external::Suspended;
use super::fail::Fail;
use super::spec::{self, BindingSpec};

impl Envelope {
    /// Seal `payload` (already encoded) as the request `binding` describes.
    /// Not for an external signer: that seals in two steps.
    pub(crate) fn seal_request(
        &self,
        payload: &[u8],
        binding: &BindingSpec,
    ) -> Result<Vec<u8>, Fail> {
        if self.external {
            return Err(Fail::misuse(
                "an external signer seals in two steps: beginSealRequest, then finish",
            ));
        }
        let call = binding.to_call(&self.audience)?;
        let sealed = ready(self.inner.seal_request(payload, &call.request()))
            .ok_or_else(|| Fail::misuse("the signer did not complete"))??;
        Ok(sealed.to_vec())
    }

    /// Seal up to the external signer.
    pub(crate) fn begin_seal(
        &self,
        payload: &[u8],
        binding: &BindingSpec,
    ) -> Result<Suspended, Fail> {
        if !self.external {
            return Err(Fail::misuse("only an external signer seals in two steps"));
        }
        let call = binding.to_call(&self.audience)?;
        let inner = self.inner.clone();
        let payload = payload.to_vec();
        Suspended::start(Box::pin(async move {
            inner.seal_request(&payload, &call.request()).await
        }))
    }

    /// The payload-type selector headers `binding` needs sent, as (name,
    /// value) pairs: `Cratestack-Payload-Type` when `payloadType` is not
    /// CBOR, `Cratestack-Payload-Accept` when `payloadAccept` is not just CBOR.
    pub(crate) fn selector_headers(
        &self,
        binding: &BindingSpec,
    ) -> Result<Vec<(String, String)>, Fail> {
        let call = binding.to_call(&self.audience)?;
        Ok(call
            .selector_headers()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect())
    }

    /// Open the response `body` to `sealed_request` (the exact bytes that
    /// were sent), which came back with HTTP `status`.
    /// `response_payload_type` is the response's `Cratestack-Payload-Type`
    /// header (`None`: CBOR); it must be one of the binding's accepted
    /// types. Every failed check rejects the same way, with no detail.
    pub(crate) fn open_response(
        &self,
        body: &[u8],
        binding: &BindingSpec,
        sealed_request: &[u8],
        status: u16,
        response_payload_type: Option<&str>,
    ) -> Result<OpenedResponse, Fail> {
        let call = binding.to_call(&self.audience)?;
        let bind = call.response_for(sealed_request, status, response_payload_type)?;
        let opened: Opened = ready(
            self.inner
                .open_response(Bytes::copy_from_slice(body), &bind),
        )
        .ok_or_else(|| Fail::misuse("the key resolver did not complete"))??;
        Ok(OpenedResponse {
            payload: opened.payload.to_vec(),
            payload_type: bind.payload_media_type.to_string(),
            kid: opened.kid.to_vec(),
            alg: spec::alg_name(opened.alg)?,
            thumbprint: opened.thumbprint.to_vec(),
        })
    }
}
