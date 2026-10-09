//! The answer to a sealed call: [`PendingResponse::open`] (cratestack#1168).

use std::borrow::Cow;

use bytes::Bytes;
use cratestack_core::{
    Binding, CratestackError, DEFAULT_PAYLOAD_MEDIA_TYPE, MAX_PAYLOAD_TYPE_LEN,
    PAYLOAD_TYPE_HEADER, RequestDigest, ResponseBinding, parse_payload_type,
};
use reqwest::header::{CONTENT_TYPE, HeaderMap};

use super::ClientEnvelope;
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;

/// What a sealed call is waiting for: the request it answers, and the
/// payload types it asked to be answered in. Made by
/// [`ClientEnvelope::seal_call`](super::ClientEnvelope::seal_call), spent by
/// [`open`](Self::open).
#[derive(Debug)]
pub struct PendingResponse {
    envelope: ClientEnvelope,
    binding: Binding<'static>,
    request: RequestDigest,
    advertised: Vec<String>,
}

/// An answer that verified: the payload and the type it was sealed under.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenedResponse {
    /// The type the response binding names, always one the call asked for.
    pub payload_type: String,
    /// The verified payload.
    pub body: Bytes,
}

impl PendingResponse {
    pub(super) fn new(
        envelope: ClientEnvelope,
        binding: Binding<'static>,
        request: RequestDigest,
        advertised: Vec<String>,
    ) -> Self {
        Self {
            envelope,
            binding,
            request,
            advertised,
        }
    }

    /// Verify the answer (`status`, its `headers` and `body`) against the
    /// request this was made for.
    ///
    /// - An answer that is not a COSE message is [`EnvelopeError::Unsigned`]
    ///   whatever its status, and nothing in its body is read.
    /// - A type in `Cratestack-Payload-Type` (absent means CBOR) that the
    ///   call did not ask for is [`EnvelopeError::UnexpectedPayloadType`]:
    ///   the body is not opened, let alone decoded.
    /// - A message that fails verification, or that is sealed under another
    ///   type, status, route or request than the header and the call say, is
    ///   [`EnvelopeError::Unverified`].
    pub async fn open(
        self,
        status: u16,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<OpenedResponse, ClientError> {
        if !is_cose(headers) {
            return Err(EnvelopeError::Unsigned { status }.into());
        }
        let payload_type = self.response_type(headers)?;
        // `self` is spent by this call, so its binding is turned into the
        // answer's in place.
        let mut answer = self.binding;
        answer.payload_media_type = Cow::Borrowed(&payload_type);
        answer.response = Some(ResponseBinding {
            request: self.request,
            status,
        });
        let opened = self
            .envelope
            .cose()
            .open_response(body, &answer)
            .await
            .map_err(open_error)?;
        Ok(OpenedResponse {
            payload_type,
            body: opened.payload,
        })
    }

    /// The type the response says it is sealed under, if the call asked for
    /// it. The header is not authenticated by itself: it names the string the
    /// binding is rebuilt with, and a wrong one fails verification.
    fn response_type(&self, headers: &HeaderMap) -> Result<String, ClientError> {
        let mut values = headers.get_all(PAYLOAD_TYPE_HEADER).iter();
        let named = match (values.next(), values.next()) {
            (None, _) => DEFAULT_PAYLOAD_MEDIA_TYPE.to_owned(),
            (Some(value), None) => parse_payload_type(value.as_bytes())
                .map(str::to_owned)
                .map_err(|_| unexpected(&String::from_utf8_lossy(value.as_bytes())))?,
            (Some(_), Some(_)) => return Err(unexpected("(repeated)")),
        };
        match self.advertised.contains(&named) {
            true => Ok(named),
            false => Err(unexpected(&named)),
        }
    }
}

fn unexpected(got: &str) -> ClientError {
    EnvelopeError::UnexpectedPayloadType {
        got: got.chars().take(MAX_PAYLOAD_TYPE_LEN + 1).collect(),
    }
    .into()
}

/// Whether the response names exactly one content type, an
/// `application/cose` one.
pub(crate) fn is_cose(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return false;
    };
    value.to_str().is_ok_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or(value)
            .trim()
            .eq_ignore_ascii_case("application/cose")
    })
}

/// Verification failures are the coarse `401` (ADR 0006 §10); anything else
/// is a backend, not the message.
fn open_error(error: CratestackError) -> ClientError {
    match error {
        CratestackError::Unauthorized(_) => EnvelopeError::Unverified,
        other => EnvelopeError::Open(other),
    }
    .into()
}
