//! Every Compatible verdict of `cratestack_core::classify` is backed by the
//! real generated types (cratestack#1123): values of the old shapes encode
//! (CBOR and JSON) and decode as the new ones with the value the signer
//! encoded, and the new reply decodes as the old one. See the old fixture
//! for the list of edits.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde_json::json;

mod old {
    cratestack::include_client_schema!("tests/fixtures/contract_roundtrip_old.cstack");
}

mod new {
    cratestack::include_server_schema!("tests/fixtures/contract_roundtrip_new.cstack", db = None);
}

mod default_old {
    cratestack::include_client_schema!("tests/fixtures/contract_roundtrip_default_old.cstack");
}

mod default_new {
    cratestack::include_server_schema!(
        "tests/fixtures/contract_roundtrip_default_new.cstack",
        db = None
    );
}

fn schema(path: &str) -> cratestack_core::Schema {
    let source = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    cratestack_parser::parse_schema(&source).expect("parses")
}

fn contract(schema: &cratestack_core::Schema) -> serde_json::Value {
    let json = cratestack_core::op_contract_json(schema, "procedure.paint").expect("op");
    serde_json::from_str(&json).expect("canonical JSON")
}

#[test]
fn the_classifier_calls_the_whole_edit_compatible() {
    let verdict = cratestack_core::classify(
        &contract(&schema("contract_roundtrip_old.cstack")),
        &contract(&schema("contract_roundtrip_new.cstack")),
    );
    assert!(verdict.is_compatible(), "{verdict:?}");
}

#[test]
fn old_input_decodes_as_the_new_input_with_the_value_the_signer_encoded() {
    use old::cratestack_schema as o;
    let args = o::procedures::paint::Args {
        args: o::Paint {
            shade: o::Shade::Dark,
            note: "matte".to_owned(),
            size: 3,
        },
        coat: 2,
        gloss: 9,
    };
    let cbor = CborCodec.encode(&args).expect("encodes");
    let json_bytes = serde_json::to_vec(&args).expect("encodes");
    let decoded_cbor: new::cratestack_schema::procedures::paint::Args = CborCodec
        .decode(&cbor)
        .expect("new server decodes old CBOR");
    let decoded_json: new::cratestack_schema::procedures::paint::Args =
        serde_json::from_slice(&json_bytes).expect("new server decodes old JSON");
    for decoded in [decoded_cbor, decoded_json] {
        assert_eq!(format!("{:?}", decoded.args.shade), "Dark");
        assert_eq!(
            decoded.args.note.as_deref(),
            Some("matte"),
            "required became optional"
        );
        assert_eq!(decoded.args.size, 3);
        assert_eq!(
            decoded.args.label, None,
            "the added optional field is absent"
        );
        assert_eq!(decoded.coat, 2);
        assert_eq!(decoded.gloss, Some(9));
        assert_eq!(
            decoded.finish, None,
            "the added optional argument is absent"
        );
    }
}

#[test]
fn the_new_reply_decodes_as_the_old_one_ignoring_the_added_field() {
    use new::cratestack_schema as n;
    let reply = n::Receipt {
        total: 40,
        tone: n::Tone::Cool,
        tax: 7,
    };
    let cbor = CborCodec.encode(&reply).expect("encodes");
    let json_bytes = serde_json::to_vec(&reply).expect("encodes");
    let decoded_cbor: old::cratestack_schema::Receipt = CborCodec
        .decode(&cbor)
        .expect("old client decodes new CBOR");
    let decoded_json: old::cratestack_schema::Receipt =
        serde_json::from_slice(&json_bytes).expect("old client decodes new JSON");
    for decoded in [decoded_cbor, decoded_json] {
        assert_eq!(decoded.total, 40);
        assert_eq!(format!("{:?}", decoded.tone), "Cool");
    }
    assert_eq!(serde_json::to_value(&reply).unwrap()["tax"], json!(7));
}

/// The refused edit: a required `type` field with `@default` is not
/// defaulted on decode, so the classifier and the generated code must agree
/// that an old client's message cannot be honoured (B1 of the #1132 review).
#[test]
fn a_type_gaining_a_defaulted_required_field_is_refused_and_does_not_decode() {
    let verdict = cratestack_core::classify(
        &contract(&schema("contract_roundtrip_default_old.cstack")),
        &contract(&schema("contract_roundtrip_default_new.cstack")),
    );
    assert!(
        verdict.reasons().iter().any(|r| r.contains("@default")),
        "{verdict:?}"
    );
    use default_old::cratestack_schema as o;
    let args = o::procedures::paint::Args {
        args: o::Paint { size: 3 },
    };
    let cbor = CborCodec.encode(&args).expect("encodes");
    let json_bytes = serde_json::to_vec(&args).expect("encodes");
    let cbor_result: Result<default_new::cratestack_schema::procedures::paint::Args, _> =
        CborCodec.decode(&cbor);
    let json_result: Result<default_new::cratestack_schema::procedures::paint::Args, _> =
        serde_json::from_slice(&json_bytes);
    assert!(cbor_result.is_err(), "CBOR decoded without `level`");
    assert!(json_result.is_err(), "JSON decoded without `level`");
}
