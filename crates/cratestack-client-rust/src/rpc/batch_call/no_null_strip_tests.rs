use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Req {
    required: String,
    #[serde(default)]
    optional: Option<String>,
    #[serde(default)]
    nested: Option<Inner>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Inner {
    #[serde(default)]
    maybe: Option<String>,
    kept: String,
}

/// cratestack#677: `BatchableCall::new` no longer strips `null` object
/// entries before handing the value to the codec (that workaround for
/// the `0x80`-vs-`0xf6` mis-encoding was obsoleted by cratestack#657's
/// `serialize_unit_as_null` fix, and was actively dropping explicit
/// nullable-column clears — see `crates/cratestack-pg/tests/
/// rpc_batch_explicit_null_clear.rs` for the production-path
/// regression test). This is the narrower codec-level guarantee the
/// removal now leans on: an explicit `null`, nested or not, must
/// still reach the wire as CBOR null and decode cleanly.
#[test]
fn unstripped_null_reaches_wire_as_cbor_null_and_decodes_cleanly() {
    let value = serde_json::json!({
        "required": "x",
        "optional": null,
        "nested": { "maybe": null, "kept": "k" },
    });
    let bytes = CborCodec.encode(&value).expect("encode");
    assert!(
        bytes.contains(&0xf6),
        "an explicit null, nested or not, must reach the wire as RFC 8949 null (0xf6): {bytes:02x?}"
    );
    let decoded: Req = CborCodec
        .decode(&bytes)
        .expect("an unstripped null must decode, not error");
    assert_eq!(decoded.required, "x");
    assert_eq!(decoded.optional, None, "top-level null decodes to None");
    assert_eq!(
        decoded.nested,
        Some(Inner {
            maybe: None,
            kept: "k".to_owned(),
        }),
        "nested null decodes to None, sibling field untouched"
    );
}
