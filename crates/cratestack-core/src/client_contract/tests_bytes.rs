//! Pins the canonical bytes and digests so a derivation change is a
//! reviewed diff, not a silent one. The bytes do not depend on
//! `serde_json`'s `preserve_order` feature: the structs fix the order.

use super::tests::sample;
use super::*;

const PING_JSON: &str = concat!(
    r#"{"closure":{"enums":[],"models":[],"types":[{"fields":[{"attributes":[],"name":"note","#,
    r#""ty":{"arity":"required","generic_args":[],"ident_args":[],"int_args":[],"name":"String"}}],"#,
    r#""name":"Ping"}],"views":[]},"events":null,"key":"POST /$procs/ping","kind":"unary","#,
    r#""model":null,"procedure":{"args":[{"name":"args","ty":{"arity":"required","generic_args":[],"#,
    r#""ident_args":[],"int_args":[],"name":"Ping"}}],"attributes":[],"kind":"mutation","name":"ping","#,
    r#""return_type":{"arity":"required","generic_args":[],"ident_args":[],"int_args":[],"name":"Ping"}},"#,
    r#""transport":"rest","verb":"mutation"}"#,
);

#[test]
fn canonical_json_of_a_procedure_is_pinned() {
    assert_eq!(
        op_contract_json(&sample(), "POST /$procs/ping").unwrap(),
        PING_JSON
    );
}

#[test]
fn digest_is_the_domain_tagged_hash_of_those_bytes() {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(OP_CONTRACT_DOMAIN);
    hasher.update(PING_JSON.as_bytes());
    let expected: [u8; 32] = hasher.finalize().into();
    assert_eq!(
        op_contract_digest(&sample(), "POST /$procs/ping"),
        Some(expected)
    );
}
