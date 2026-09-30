//! Reading the server's unsigned `426` (cratestack#1123): which refusals
//! are the contract one, and what op they name.

use cratestack_core::{CONTRACT_UNSUPPORTED_CODE, CONTRACT_UNSUPPORTED_REST_CODE};
use reqwest::Method;
use reqwest::header::{CONTENT_TYPE, HeaderMap};

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;

/// The op a refusal names: the method and template on REST (the table's
/// own key, so `GET /widgets/{id}` and `DELETE /widgets/{id}` differ), the
/// op id on RPC.
pub(super) fn refused_op(method: &Method, template: &str) -> String {
    match template.starts_with('/') {
        true => format!("{method} {template}"),
        false => template.to_owned(),
    }
}

/// The two error shapes a refusal can take (`RpcErrorBody`,
/// `CratestackErrorResponse`), read for their `code` alone.
#[derive(serde::Deserialize)]
struct CodeOnly {
    code: String,
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Whether an unsigned response's body carries the contract refusal's
    /// code, in either transport's spelling. Unauthenticated, like the
    /// answer itself: it only decides which hint the caller gets.
    pub(super) fn is_contract_refusal(&self, headers: &HeaderMap, body: &[u8]) -> bool {
        let Some(content_type) = headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        self.codec
            .decode_response::<CodeOnly>(content_type, body)
            .is_ok_and(|body| {
                body.code == CONTRACT_UNSUPPORTED_CODE
                    || body.code == CONTRACT_UNSUPPORTED_REST_CODE
            })
    }
}
