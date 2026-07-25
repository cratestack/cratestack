//! proto3 scalar mapping — `docs/design/protobuf.md` §4.1-4.3, ticket
//! #169's Part C table. `Page<T>` is deliberately absent from this table:
//! it never reaches here, because [`super::synth`] monomorphizes every
//! `Page<T>` occurrence into a plain message-reference `TypeRef` before any
//! field is rendered (parser-enforced: `Page<T>` only ever appears in
//! procedure-return-type position, never on a model/type field — see
//! `crates/cratestack-parser/src/validate/type_names.rs`).

pub(super) struct MappedScalar {
    pub(super) proto_type: String,
    pub(super) needs_timestamp_import: bool,
    pub(super) trailing_comment: Option<&'static str>,
}

pub(super) fn map_scalar(name: &str) -> MappedScalar {
    match name {
        "String" | "Cuid" | "Uuid" => plain("string"),
        "Int" => plain("int64"),
        "Float" => plain("double"),
        "Boolean" => plain("bool"),
        "Bytes" => plain("bytes"),
        // No units/nanos message — `rust_decimal` round-trips exactly
        // through its string form and the schema carries no declared
        // scale for a fixed-point mapping to be sound. §4.2.
        "Decimal" => plain("string"),
        // `bytes` holding UTF-8 JSON, not `google.protobuf.Struct`, which
        // loses precision on any i64 above 2^53. §4.3.
        "Json" => MappedScalar {
            proto_type: "bytes".to_owned(),
            needs_timestamp_import: false,
            trailing_comment: Some("json"),
        },
        "DateTime" => MappedScalar {
            proto_type: "google.protobuf.Timestamp".to_owned(),
            needs_timestamp_import: true,
            trailing_comment: None,
        },
        // Enum / message / type reference: the proto identifier is the
        // schema name unchanged, same passthrough every other CrateStack
        // generator (TS, Dart) already uses.
        other => plain(other),
    }
}

fn plain(proto_type: &str) -> MappedScalar {
    MappedScalar {
        proto_type: proto_type.to_owned(),
        needs_timestamp_import: false,
        trailing_comment: None,
    }
}

#[cfg(test)]
mod tests {
    use super::map_scalar;

    #[test]
    fn decimal_maps_to_string() {
        assert_eq!(map_scalar("Decimal").proto_type, "string");
    }

    #[test]
    fn json_maps_to_bytes_with_comment() {
        let mapped = map_scalar("Json");
        assert_eq!(mapped.proto_type, "bytes");
        assert_eq!(mapped.trailing_comment, Some("json"));
    }

    #[test]
    fn datetime_maps_to_timestamp_and_needs_import() {
        let mapped = map_scalar("DateTime");
        assert_eq!(mapped.proto_type, "google.protobuf.Timestamp");
        assert!(mapped.needs_timestamp_import);
    }

    #[test]
    fn unknown_name_passes_through_as_a_reference() {
        assert_eq!(map_scalar("Order").proto_type, "Order");
    }
}
