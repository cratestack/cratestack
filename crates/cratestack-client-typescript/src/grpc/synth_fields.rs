//! Field lists for the messages this generator synthesizes rather than
//! reads straight off a schema `Model`/`TypeDecl` — `Create<M>Input`/
//! `Update<M>Input` (scalar projections of a real model) and the two
//! framework-level helper messages, `PageInfo`/`PageOf<M>`
//! (`cratestack-proto::emit::synth_page`'s Rust-side counterpart). Split
//! out of `messages.rs` to stay under the repo's 200-LoC convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, SourceSpan, TypeArity, TypeRef};

/// `Create<M>Input`: same field set the REST/RPC generator uses
/// (`crate::types::scalar_model_fields`, minus generated-on-create), so
/// the gRPC wire shape matches the already-generated `models.ts`
/// interface field-for-field. `model_names` must be every model name in
/// the *schema*, not just this one — `scalar_model_fields`'s relation
/// filter checks `field.ty.name` against it, and a relation almost always
/// points at a *different* model name than the one being built.
pub(super) fn scalar_fields_for_create(model: &Model, model_names: &BTreeSet<&str>) -> Vec<Field> {
    crate::types::scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !crate::types::is_generated_on_create(field))
        .cloned()
        .collect()
}

pub(super) fn scalar_fields_for_update(model: &Model, model_names: &BTreeSet<&str>) -> Vec<Field> {
    crate::types::scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !crate::types::is_primary_key(field))
        .cloned()
        .collect()
}

/// `PageInfo`'s fixed shape (`cratestack-proto::emit::synth_page`) — field
/// *names* here stay the real snake_case ones registered in the lock;
/// `collect_from_fields`'s `camel_case_properties` flag is what turns
/// them into `hasNextPage`/`hasPreviousPage` on the generated TS side.
pub(super) fn page_info_wire_fields() -> Vec<Field> {
    vec![
        synthetic_field("limit", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("offset", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("has_next_page", scalar_ty("Boolean", TypeArity::Required)),
        synthetic_field(
            "has_previous_page",
            scalar_ty("Boolean", TypeArity::Required),
        ),
    ]
}

pub(super) fn page_of_wire_fields(model_name: &str) -> Vec<Field> {
    vec![
        synthetic_field("items", scalar_ty(model_name, TypeArity::List)),
        synthetic_field("total_count", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("page_info", scalar_ty("PageInfo", TypeArity::Required)),
    ]
}

pub(crate) fn synthetic_field(name: &str, ty: TypeRef) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: synthetic_span(),
        ty,
        attributes: Vec::new(),
        span: synthetic_span(),
    }
}

pub(crate) fn scalar_ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: vec![],
    }
}

fn synthetic_span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}
