//! Synthesizes the three per-model gRPC-only request-wrapper messages —
//! `<Model>RpcPkInput`, `<Model>RpcUpdateInput`, `<Model>RpcListInput` —
//! plus their two shared helper messages, `StringList` and
//! `RpcListPredicate`. These exist only for `transport grpc` schemas
//! (`emit::service`'s CRUD methods reference them); REST/RPC schemas never
//! see them — [`synthesize_rpc_inputs`] is a no-op unless
//! `schema.transport == TransportStyle::Grpc`.
//!
//! They mirror hand-written Rust types with no `.cstack` counterpart —
//! `cratestack_axum::rpc::inputs::{RpcPkInput, RpcUpdateInput, RpcListInput,
//! RpcListPredicate}` — the same "translate a fixed Rust shape into proto"
//! category of work ticket #169 already did for `PageInfo`
//! (`synth_page.rs`).
//!
//! Simplification, documented per ticket #170: every field here goes
//! through the same universal proto3 `optional` rule as everything else
//! this crate emits (`docs/design/protobuf.md` §4.4), rather than trying to
//! mirror the Rust struct's `#[serde(skip_serializing_if = ...)]`
//! attributes field-by-field (e.g. `RpcListPredicate.key`/`.value` are
//! plain non-`Option<T>` `String`s in Rust, but render as `optional string`
//! here like every other message field). No runtime consumes this shape
//! yet (ticket #171); getting the structure right — the message exists, is
//! referenced correctly by `emit::service`, and round-trips through
//! `protoc` — matters more than perfect fidelity to the Rust struct's serde
//! attributes.

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema, SourceSpan, TransportStyle, TypeArity, TypeRef};

use super::error::ProtoEmitError;
use super::mirror::model_primary_key_field;
use super::synth::insert_synth;

pub(super) fn synthesize_rpc_inputs(
    schema: &Schema,
    occupied: &mut BTreeMap<String, &'static str>,
    extra: &mut BTreeMap<String, Vec<Field>>,
) -> Result<(), ProtoEmitError> {
    if schema.transport != TransportStyle::Grpc {
        return Ok(());
    }

    let models_with_pk: Vec<(&str, &Field)> = schema
        .models
        .iter()
        .filter_map(|model| model_primary_key_field(model).map(|pk| (model.name.as_str(), pk)))
        .collect();
    if models_with_pk.is_empty() {
        return Ok(());
    }

    // Emitted once per file, not once per model — every `<Model>RpcListInput`
    // below references both by name.
    insert_synth(
        occupied,
        extra,
        "StringList".to_owned(),
        string_list_fields(),
    )?;
    insert_synth(
        occupied,
        extra,
        "RpcListPredicate".to_owned(),
        rpc_list_predicate_fields(),
    )?;

    for (name, pk) in models_with_pk {
        insert_synth(
            occupied,
            extra,
            format!("{name}RpcPkInput"),
            vec![id_field(pk)],
        )?;
        insert_synth(
            occupied,
            extra,
            format!("{name}RpcUpdateInput"),
            vec![id_field(pk), patch_field(name, pk)],
        )?;
        insert_synth(
            occupied,
            extra,
            format!("{name}RpcListInput"),
            rpc_list_input_fields(pk),
        )?;
    }
    Ok(())
}

/// Mirrors `RpcPkInput<Pk>::id`: same span as the model's own `@id` field so
/// a `MissingLockEntry`/diagnostic pointing at this synthesized field still
/// lands somewhere meaningful in the source.
fn id_field(pk: &Field) -> Field {
    Field {
        docs: vec![],
        name: "id".to_owned(),
        name_span: pk.name_span,
        ty: pk.ty.clone(),
        attributes: Vec::new(),
        span: pk.span,
    }
}

/// Mirrors `RpcUpdateInput<Pk, Patch>::patch`, referencing the
/// already-synthesized `Update<Model>Input` message by name (always
/// present — ticket #169 synthesizes it unconditionally for every model).
fn patch_field(model_name: &str, pk: &Field) -> Field {
    Field {
        docs: vec![],
        name: "patch".to_owned(),
        name_span: pk.name_span,
        ty: scalar_ty(&format!("Update{model_name}Input"), TypeArity::Required),
        attributes: Vec::new(),
        span: pk.span,
    }
}

/// Mirrors `RpcListInput`'s 9 fields. `include_fields` carries a
/// `map<string, StringList>` marker type — `emit::message::render_message`
/// special-cases the field by name to render the map syntax rather than
/// going through the ordinary scalar/message renderer, the same way
/// `PageInfo`'s bool fields bypass the universal-optional renderer.
fn rpc_list_input_fields(pk: &Field) -> Vec<Field> {
    let span = pk.span;
    vec![
        synthetic_field("limit", scalar_ty("Int", TypeArity::Optional), span),
        synthetic_field("offset", scalar_ty("Int", TypeArity::Optional), span),
        synthetic_field("fields", scalar_ty("String", TypeArity::List), span),
        synthetic_field("include", scalar_ty("String", TypeArity::List), span),
        synthetic_field(
            "include_fields",
            scalar_ty("StringList", TypeArity::Required),
            span,
        ),
        synthetic_field("sort", scalar_ty("String", TypeArity::Optional), span),
        synthetic_field("where_expr", scalar_ty("String", TypeArity::Optional), span),
        synthetic_field("or", scalar_ty("String", TypeArity::Optional), span),
        synthetic_field(
            "filters",
            scalar_ty("RpcListPredicate", TypeArity::List),
            span,
        ),
    ]
}

fn string_list_fields() -> Vec<Field> {
    vec![synthetic_field(
        "values",
        scalar_ty("String", TypeArity::List),
        synthetic_span(),
    )]
}

fn rpc_list_predicate_fields() -> Vec<Field> {
    let span = synthetic_span();
    vec![
        synthetic_field("key", scalar_ty("String", TypeArity::Required), span),
        synthetic_field("value", scalar_ty("String", TypeArity::Required), span),
    ]
}

fn scalar_ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: vec![],
        int_args: Vec::new(),
    }
}

fn synthetic_field(name: &str, ty: TypeRef, span: SourceSpan) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: span,
        ty,
        attributes: Vec::new(),
        span,
    }
}

fn synthetic_span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}
