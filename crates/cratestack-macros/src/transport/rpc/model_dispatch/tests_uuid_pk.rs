//! cratestack#1359: a `Uuid` primary key decodes its `{ id }` RPC frame
//! through `LenientUuid` (text or 16 bytes under CBOR); every other key
//! type keeps decoding into its own Rust type.

use super::generate_model_rpc_dispatch_arms;

fn rendered(pk_type: &str) -> String {
    let source = format!("model Widget {{\n  id {pk_type} @id\n}}\n");
    let model = cratestack_parser::parse_schema(&source)
        .expect("fixture parses")
        .models
        .remove(0);
    generate_model_rpc_dispatch_arms(&model)
        .iter()
        .map(|arm| arm.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn uuid_key_frames_decode_through_lenient_uuid_and_convert_back() {
    let out = rendered("Uuid");
    for frame in ["RpcGetInput", "RpcPkInput", "RpcUpdateInput"] {
        assert!(
            out.contains(&format!("{frame} < :: cratestack :: LenientUuid")),
            "{frame} must decode LenientUuid: {out}"
        );
    }
    assert!(out.contains("input . id . into ()"), "{out}");
    assert!(!out.contains("RpcGetInput < :: cratestack :: uuid"), "{out}");
}

#[test]
fn non_uuid_keys_are_untouched() {
    let out = rendered("Int");
    assert!(!out.contains("LenientUuid"), "{out}");
    assert!(out.contains("RpcGetInput < i64 >"), "{out}");
}
