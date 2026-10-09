//! AAD vectors for the payload type (cratestack#1168): a CBOR exchange is
//! byte-identical to what 0.15.3 produced, and a form request / JSON response
//! differ from it in element 8 and nowhere else. `payload-types.json` holds
//! hex only, for the other language bindings; the file is re-derived here and
//! only rewritten when `CRATESTACK_COSE_WRITE_VECTORS=1` is set.

mod common;

use std::borrow::Cow;
use std::path::PathBuf;

use common::{answering, hex, rest_request};
use cratestack_core::Binding;
use cratestack_cose::{external_aad, request_digest};

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";

fn typed(mut bind: Binding<'static>, payload_type: &'static str) -> Binding<'static> {
    bind.payload_media_type = Cow::Borrowed(payload_type);
    bind
}

fn aad(bind: &Binding<'static>) -> Vec<u8> {
    external_aad(bind).expect("aad")
}

/// (name, request payload type, response payload type)
const EXCHANGES: &[(&str, &str, &str)] = &[
    ("cbor-in-cbor-out", "application/cbor", "application/cbor"),
    ("form-in-json-out", FORM, JSON),
    ("json-in-json-out", JSON, JSON),
    ("cbor-in-json-out", "application/cbor", JSON),
];

fn derive() -> serde_json::Value {
    let digest = request_digest(b"sealed request bytes");
    let cases: Vec<_> = EXCHANGES
        .iter()
        .map(|(name, request_type, response_type)| {
            let request = typed(rest_request(), request_type);
            let response = answering(&typed(rest_request(), response_type), digest, 200);
            serde_json::json!({
                "name": name,
                "request_payload_type": request_type,
                "response_payload_type": response_type,
                "request_aad": hex(&aad(&request)),
                "response_aad": hex(&aad(&response)),
            })
        })
        .collect();
    serde_json::json!({
        "_comment": "AAD vectors for the payload type (cratestack#1168). Binding version 2, unchanged: only element 8 (`payload_type`) varies. A request binds the request payload's type; a response binds the response payload's own type. The request binding is rest_request() of the unary vectors, the response binding answers the request whose sealed bytes are the ASCII string `sealed request bytes`, status 200. `cbor-in-cbor-out` is byte-identical to what 0.15.3 produced.",
        "sealed_request": hex(b"sealed request bytes"),
        "cases": cases,
    })
}

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/payload-types.json")
}

#[test]
fn the_payload_type_vectors_match() {
    let derived = derive();
    let rendered = serde_json::to_string_pretty(&derived).expect("json") + "\n";
    if std::env::var("CRATESTACK_COSE_WRITE_VECTORS").as_deref() == Ok("1") {
        std::fs::write(path(), &rendered).expect("write vectors");
    }
    let on_disk = std::fs::read_to_string(path()).expect("payload-types.json is checked in");
    let on_disk: serde_json::Value = serde_json::from_str(&on_disk).expect("vector json");
    assert_eq!(on_disk, derived);
}

#[test]
fn the_cbor_exchange_is_the_unchanged_binding_and_each_type_changes_it() {
    let baseline = aad(&rest_request());
    assert_eq!(rest_request().payload_media_type, "application/cbor");
    let form = aad(&typed(rest_request(), FORM));
    let json = aad(&typed(rest_request(), JSON));
    assert_ne!(baseline, form);
    assert_ne!(baseline, json);
    assert_ne!(form, json);
    // Element 8 is the text string that names the type, and the AAD carries
    // each type's bytes verbatim.
    for (aad, media_type) in [
        (&baseline, "application/cbor"),
        (&form, FORM),
        (&json, JSON),
    ] {
        assert!(
            aad.windows(media_type.len())
                .any(|window| window == media_type.as_bytes()),
            "{media_type}"
        );
    }
}
