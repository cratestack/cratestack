use axum::http::HeaderMap;
use cratestack_core::{CoolCodec, CoolError};

use crate::transport::{
    validate_transport_accept_header, validate_transport_content_type_header,
};

pub fn validate_codec_response_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)
}

pub fn validate_codec_request_headers<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_accept_header::<C>(headers)?;
    validate_content_type_header::<C>(headers)
}

pub(crate) fn validate_accept_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_accept_header(headers, &[C::CONTENT_TYPE])
}

pub(crate) fn validate_content_type_header<C>(headers: &HeaderMap) -> Result<(), CoolError>
where
    C: CoolCodec,
{
    validate_transport_content_type_header(headers, &[C::CONTENT_TYPE])
}
