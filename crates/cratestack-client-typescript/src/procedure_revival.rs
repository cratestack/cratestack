//! How a procedure's return type is revived on decode, split out of
//! `wire_shapes.rs` (200-LoC ceiling). See that module for the field-level
//! scheme this mirrors.

use cratestack_core::{TypeArity, TypeRef};

/// How a procedure's return type should be revived on decode
/// (cratestack#498 F2) — see `ProcedureView::revival_kind`'s doc
/// comment (`crate::procedure_views`) for the generated-code side of this.
pub(crate) enum ProcedureRevival {
    /// The return type is a bare scalar needing revival — `Decimal`,
    /// `BigInt`, `Uuid` or `Bytes`, at any arity. `Page<T>` is not a validated shape for
    /// either (`Page<T>`'s item must be a declared model or `type`,
    /// `cratestack-parser::validate::type_names`), so this can only ever
    /// be the bare name, `?`, or `[]`.
    Scalar(ScalarRevival),
    /// The (`Page<T>`-unwrapped) base type names a declared model or
    /// `type` — decode via that type's own shape (`String` is the shape
    /// name; a name with no registry entry, e.g. a plain scalar or enum
    /// return, is `reviveWireFields`'s documented no-op fast path).
    /// `paged` is `true` when the original return type was `Page<T>` —
    /// the generated call site uses `revivePagedWireFields` instead of
    /// `reviveWireFields` for that case (the decoded envelope's own
    /// keys, `items`/`totalCount`/`pageInfo`, are never themselves `T`'s
    /// fields, so `T`'s shape must be applied to `.items`, not the
    /// envelope).
    Shape { shape_name: String, paged: bool },
}

/// Which bare-scalar revival a [`ProcedureRevival::Scalar`] return needs.
/// Rendered straight into the generated `reviveWireScalar(value, "...")`
/// call as its second argument, so these strings are a wire contract with
/// `models.ts.j2`'s runtime — keep the two in lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarRevival {
    /// `Decimal` / `Decimal?` / `Decimal[]` — a wire string (or array of
    /// them) becomes a `Decimal` instance.
    Decimal,
    /// `BigInt` / `BigInt?` / `BigInt[]` — a canonical decimal wire string
    /// (or array of them) becomes a `bigint`; anything else throws.
    BigInt,
    /// `Uuid` / `Uuid?` / `Uuid[]` — a 16-byte `Uint8Array` (or an array of
    /// them) becomes the canonical hyphenated string; a string passes
    /// through; anything else throws (cratestack#1359).
    Uuid,
    /// `Bytes` / `Bytes?` — one wire integer array becomes one
    /// `Uint8Array`.
    Bytes,
    /// `Bytes[]` — a wire array *of* integer arrays becomes
    /// `Uint8Array[]`. Distinct from [`Self::Bytes`] for the same
    /// arity-ambiguity reason the field-level keys are split; see the
    /// module doc.
    BytesList,
}

impl ScalarRevival {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ScalarRevival::Decimal => "decimal",
            ScalarRevival::BigInt => "bigint",
            ScalarRevival::Uuid => "uuid",
            ScalarRevival::Bytes => "bytes",
            ScalarRevival::BytesList => "bytesList",
        }
    }
}

/// Classifies `return_type` per [`ProcedureRevival`].
pub(crate) fn procedure_revival(return_type: &TypeRef) -> ProcedureRevival {
    let paged = return_type.is_page();
    let base = return_type.page_item().unwrap_or(return_type);
    match base.name.as_str() {
        "Decimal" => ProcedureRevival::Scalar(ScalarRevival::Decimal),
        "BigInt" => ProcedureRevival::Scalar(ScalarRevival::BigInt),
        "Uuid" => ProcedureRevival::Scalar(ScalarRevival::Uuid),
        "Bytes" if matches!(base.arity, TypeArity::List) => {
            ProcedureRevival::Scalar(ScalarRevival::BytesList)
        }
        "Bytes" => ProcedureRevival::Scalar(ScalarRevival::Bytes),
        _ => ProcedureRevival::Shape {
            shape_name: base.name.clone(),
            paged,
        },
    }
}
