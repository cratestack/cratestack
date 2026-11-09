//! What a verified response resolves to in JavaScript, and the
//! `Cratestack-Contract` header helper.

use cratestack_cose::Opened;
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use super::convert;
use super::error::misuse;

pub(super) fn opened_object(opened: &Opened, payload_type: &str) -> Result<JsValue, JsValue> {
    let object = Object::new();
    let set = |name: &str, value: JsValue| {
        // Setting a property on a fresh plain object cannot fail.
        let _ = Reflect::set(&object, &name.into(), &value);
    };
    set("payload", Uint8Array::from(opened.payload.as_ref()).into());
    set("payloadType", payload_type.into());
    set("kid", Uint8Array::from(&opened.kid[..]).into());
    set("alg", convert::alg_name(opened.alg)?.into());
    set(
        "thumbprint",
        Uint8Array::from(&opened.thumbprint[..]).into(),
    );
    Ok(object.into())
}

/// The `Cratestack-Contract` header value for a 32-byte op contract digest
/// (11 characters of unpadded base64url). The header is unbound; send it
/// with every sealed call.
#[wasm_bindgen(js_name = contractHeaderValue)]
pub fn contract_header_value(contract_sha: &[u8]) -> Result<String, JsValue> {
    let digest: [u8; 32] = contract_sha
        .try_into()
        .map_err(|_| misuse("`contractSha` is 32 bytes"))?;
    Ok(cratestack_cose::contract_header_value(&digest))
}

/// The canonical form of a raw query string (the part after `?`, without
/// it): the form `binding.query` must carry for the seal to match what the
/// server rebuilds. Pairs ordered by key and re-encoded, by
/// `cratestack_core::canonical_query`, so no client re-implements it.
#[wasm_bindgen(js_name = canonicalQuery)]
pub fn canonical_query(query: &str) -> String {
    cratestack_core::canonical_query(Some(query))
}
