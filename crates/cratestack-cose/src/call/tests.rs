use cratestack_core::RequestKind;

use super::*;

fn call() -> CallBinding {
    CallBinding {
        audience: "payments".to_owned(),
        method: "PUT".to_owned(),
        route: "/accounts/{account_id}/payments/{id}".to_owned(),
        path_params: vec!["acc_42".to_owned(), "pay_7".to_owned()],
        query: Some("dry_run=false".to_owned()),
        contract_sha: [9; 32],
        idempotency_key: Some("idem-7f3a".to_owned()),
        if_match: Some("\"3\"".to_owned()),
    }
}

#[test]
fn the_request_binding_carries_every_input() {
    let call = call();
    let bind = call.request();
    assert_eq!(bind.audience, "payments");
    assert_eq!(bind.method, "PUT");
    assert_eq!(bind.route, "/accounts/{account_id}/payments/{id}");
    assert_eq!(
        bind.path_params.iter().collect::<Vec<_>>(),
        ["acc_42", "pay_7"]
    );
    assert_eq!(bind.query.as_deref(), Some("dry_run=false"));
    assert_eq!(bind.contract_sha, [9; 32]);
    assert_eq!(bind.payload_media_type, "application/cbor");
    assert_eq!(
        bind.bound_headers.idempotency_key.as_deref(),
        Some("idem-7f3a")
    );
    assert_eq!(bind.bound_headers.if_match.as_deref(), Some("\"3\""));
    assert!(bind.response.is_none());
}

#[test]
fn the_query_is_canonical_and_an_empty_one_is_none() {
    let mut call = call();
    call.query = Some("z=9&a=2&a=1".to_owned());
    assert_eq!(call.request().query.as_deref(), Some("a=2&a=1&z=9"));
    for empty in [None, Some(String::new())] {
        call.query = empty;
        assert_eq!(call.request().query, None);
    }
}

#[test]
fn the_response_binding_digests_the_sealed_request() {
    let call = call();
    let bind = call.response(b"sealed", 404);
    let response = bind.response.expect("a response binding");
    assert_eq!(response.status, 404);
    assert_eq!(response.request, request_digest(b"sealed"));
    assert_eq!(response.request.kind, RequestKind::Signed);
    // Everything else is the request's.
    assert_eq!(bind.route, call.request().route);
    assert_eq!(bind.bound_headers, call.request().bound_headers);
}

#[test]
fn the_contract_header_value_is_the_digest_prefix() {
    let digest: [u8; 32] = core::array::from_fn(|i| u8::try_from(i).unwrap());
    // bytes 00..07 as unpadded base64url.
    assert_eq!(contract_header_value(&digest), "AAECAwQFBgc");
}
