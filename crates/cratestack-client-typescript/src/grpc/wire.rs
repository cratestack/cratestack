//! Field-level wire-kind mapping — the TypeScript-generation-time mirror of
//! `cratestack-proto::emit::scalar::map_scalar` (`docs/design/protobuf.md`
//! §4.1). Reimplemented locally rather than depended on: that function
//! lives in a private `cratestack-proto::emit` module (internal to the
//! `.proto`-text emitter), and the "small pure mapping table gets
//! reimplemented per crate" convention is already established by
//! `cratestack-proto::casing` itself (see its module doc) for exactly this
//! reason — a ~10-line `match` isn't worth destabilizing an internal
//! module's visibility for.
//!
//! Unlike `map_scalar`, this also classifies enum and message references
//! (`map_scalar` treats both as an opaque proto type-name passthrough,
//! which is enough for `.proto` text but not enough for a wire encoder,
//! which needs to know *how* to encode a reference — varint for an enum,
//! length-delimited + recursive encode for a message).

use std::collections::BTreeSet;

use serde::Serialize;

use cratestack_core::{TypeArity, TypeRef};

/// How a field's value is written to the wire — the bounded, known set
/// `docs/design/protobuf.md` §4.1 maps `.cstack` scalars onto. Serialized
/// as its lowercase tag so the runtime's data-driven `encodeMessage`/
/// `decodeMessage` can switch on it directly (`grpc-web-runtime.ts.j2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GrpcWireKind {
    /// `Int` -> proto3 `int64` — varint, two's-complement (not zigzag;
    /// `.cstack` never declares `sint64`).
    Int64,
    /// `Float` -> proto3 `double` — 8-byte little-endian (wire type 1).
    Double,
    /// `Boolean` -> proto3 `bool` — varint 0/1.
    Bool,
    /// `String`/`Cuid`/`Uuid`/`Decimal` -> proto3 `string` — UTF-8,
    /// length-delimited.
    String,
    /// `Bytes`/`Json` -> proto3 `bytes` — raw, length-delimited.
    Bytes,
    /// `DateTime` -> `google.protobuf.Timestamp` — a fixed 2-field nested
    /// message (`seconds: int64 = 1`, `nanos: int32 = 2`), handled by the
    /// runtime as a built-in message shape rather than a generated
    /// descriptor table.
    Timestamp,
    /// A `.cstack` enum — proto3 enum, varint, encoded/decoded through a
    /// generated name<->number lookup table (`docs/design/protobuf.md`
    /// §4.5's synthetic `_UNSPECIFIED = 0`).
    Enum,
    /// A model, `type` block, or synthesized message (`PageInfo`,
    /// `StringList`, ...) — length-delimited, recursively
    /// encoded/decoded against that message's own descriptor table.
    Message,
}

pub(crate) struct MappedWireField {
    pub(crate) kind: GrpcWireKind,
    /// Set only when `kind` is `Enum` or `Message` — the referenced
    /// type's name, used to look up its descriptor/lookup table at
    /// codegen time and to name the generated TS symbol.
    pub(crate) ref_name: Option<String>,
}

/// Maps a field's `.cstack` scalar/reference name to its wire kind.
/// `enum_names` distinguishes an enum reference (varint) from a message
/// reference (length-delimited) — both are otherwise indistinguishable
/// "unknown name" passthroughs, mirroring `map_scalar`'s own `other =>
/// plain(other)` branch.
pub(crate) fn map_wire_field(ty_name: &str, enum_names: &BTreeSet<&str>) -> MappedWireField {
    let kind = match ty_name {
        "String" | "Cuid" | "Uuid" | "Decimal" => GrpcWireKind::String,
        "Int" => GrpcWireKind::Int64,
        "Float" => GrpcWireKind::Double,
        "Boolean" => GrpcWireKind::Bool,
        "Bytes" | "Json" => GrpcWireKind::Bytes,
        "DateTime" => GrpcWireKind::Timestamp,
        other if enum_names.contains(other) => GrpcWireKind::Enum,
        _ => GrpcWireKind::Message,
    };
    let ref_name =
        matches!(kind, GrpcWireKind::Enum | GrpcWireKind::Message).then(|| ty_name.to_owned());
    MappedWireField { kind, ref_name }
}

/// A single field on a [`super::messages::GrpcMessageView`] — the runtime's
/// unit of data-driven encode/decode. `repeated` covers `TypeArity::List`;
/// every other arity (`Required`/`Optional`) is presence-tracked the same
/// way on the wire (proto3 explicit `optional`, `docs/design/protobuf.md`
/// §4.4 — CrateStack's universal-optional rule makes the Required/Optional
/// distinction a documentation-only concern, not a wire behavior one), so
/// this descriptor doesn't carry arity beyond the repeated/scalar split.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcFieldDescriptor {
    pub(crate) property: String,
    pub(crate) number: i32,
    pub(crate) kind: GrpcWireKind,
    pub(crate) repeated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ref_name: Option<String>,
    /// True only for the handful of fields that use proto3 *implicit*
    /// presence instead of CrateStack's universal-optional rule (today:
    /// `PageInfo.has_next_page`/`has_previous_page` —
    /// `cratestack-proto::emit::message::render_page_info`'s own
    /// special case, "bools are never absent on this one message"). For
    /// those, an absent wire tag means the real value *is* the type's
    /// zero value (`false`/`0`/`""`), not "not set" — decoding must fill
    /// it in rather than leaving the property `undefined`. Every other
    /// field uses explicit presence (`docs/design/protobuf.md` §4.4):
    /// absent genuinely means "not provided" (partial `fields`/`include`
    /// projection), so the property stays `undefined`, matching the
    /// optional TS field it's decoded into.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) defaults_when_absent: bool,
}

pub(crate) fn build_field_descriptor(
    property: &str,
    ty: &TypeRef,
    number: i32,
    enum_names: &BTreeSet<&str>,
) -> GrpcFieldDescriptor {
    let mapped = map_wire_field(&ty.name, enum_names);
    GrpcFieldDescriptor {
        property: property.to_owned(),
        number,
        kind: mapped.kind,
        repeated: ty.arity == TypeArity::List,
        ref_name: mapped.ref_name,
        defaults_when_absent: false,
    }
}
