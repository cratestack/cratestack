//! `Page<T>` monomorphization + the one shared `PageInfo` — split out of
//! `synth.rs` to stay under the repo's 200-LoC file convention.

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema, SourceSpan, TransportStyle, TypeArity, TypeRef};

use super::error::ProtoEmitError;
use super::mirror::model_primary_key_field;
use super::synth::insert_synth;

/// `Page<T>` is parser-restricted to procedure-return-type position (see
/// `cratestack-parser/src/validate/type_names.rs`), so scanning procedure
/// return types is exhaustive — no need to also scan model/type fields.
pub(super) fn synthesize_pages(
    schema: &Schema,
    occupied: &mut BTreeMap<String, &'static str>,
    extra: &mut BTreeMap<String, Vec<Field>>,
) -> Result<(), ProtoEmitError> {
    let mut page_items: BTreeMap<String, TypeRef> = BTreeMap::new();
    for procedure in &schema.procedures {
        if !procedure.return_type.is_page() {
            continue;
        }
        let item = procedure
            .return_type
            .page_item()
            .expect("validated Page<T> return type carries an item type");
        page_items
            .entry(page_message_name(item))
            .or_insert_with(|| item.clone());
    }

    // A `transport grpc` schema's `list` verb has an implicit `Page<Model>`
    // response even though `Page<T>` syntax never appears written on a
    // model field — `Page<T>` is parser-restricted to procedure-return
    // position (see the doc comment on this fn). That's a schema-syntax
    // restriction, not a semantic one: `op_descriptors.rs`'s own
    // `output_ty: &page_ty` where `page_ty = format!("Page<{model_name}>")`
    // treats every model's `list` op as returning `Page<Model>` regardless
    // — see `docs/design/protobuf.md`'s ticket #170 spec. So every model
    // with a `list` verb (i.e. every model with a primary key, mirroring
    // `emit::service`'s own gate) needs `PageOf<Model>` too, deduplicated
    // against any procedure that already returns `Page<Model>` by going
    // through the same `page_items` map.
    if schema.transport == TransportStyle::Grpc {
        for model in &schema.models {
            if model_primary_key_field(model).is_none() {
                continue;
            }
            let item = TypeRef {
                name: model.name.clone(),
                name_span: model.name_span,
                arity: TypeArity::Required,
                generic_args: vec![],
            };
            page_items.entry(page_message_name(&item)).or_insert(item);
        }
    }

    if page_items.is_empty() {
        return Ok(());
    }

    insert_synth(occupied, extra, "PageInfo".to_owned(), page_info_fields())?;
    for (message_name, item) in page_items {
        insert_synth(occupied, extra, message_name, page_of_fields(&item))?;
    }
    Ok(())
}

/// `pub` (not `pub(super)`) since ticket #208: `emit::mod` re-exports
/// this so `cratestack-macros::include::server::grpc::procedures` can
/// compute the exact same `<Base>Output.result` field type this crate's
/// own [`super::synth::synthesize_messages`] already used to number that
/// field in the `.pb.lock` — reusing the monomorphization rule rather
/// than re-deriving `Page<T>` -> `PageOf<Item>` a second time.
pub fn monomorphize_return_type(return_type: &TypeRef) -> TypeRef {
    if !return_type.is_page() {
        return return_type.clone();
    }
    let item = return_type
        .page_item()
        .expect("validated Page<T> return type carries an item type");
    TypeRef {
        name: page_message_name(item),
        name_span: return_type.name_span,
        arity: TypeArity::Required,
        generic_args: vec![],
    }
}

fn page_message_name(item: &TypeRef) -> String {
    format!("PageOf{}", item.name)
}

fn page_of_fields(item: &TypeRef) -> Vec<Field> {
    vec![
        synthetic_field(
            "items",
            TypeRef {
                name: item.name.clone(),
                name_span: item.name_span,
                arity: TypeArity::List,
                generic_args: vec![],
            },
        ),
        synthetic_field("total_count", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("page_info", scalar_ty("PageInfo", TypeArity::Required)),
    ]
}

/// Field arity here documents the domain shape
/// (`cratestack_core::page::PageInfo`) but does not drive rendering:
/// `emit::message::render_message` hard-codes `PageInfo`'s own presence
/// rule (bools are never `optional`; `emit::field::render_field` is not
/// used for this message at all).
fn page_info_fields() -> Vec<Field> {
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

fn scalar_ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: synthetic_span(),
        arity,
        generic_args: vec![],
    }
}

fn synthetic_field(name: &str, ty: TypeRef) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: synthetic_span(),
        ty,
        attributes: Vec::new(),
        span: synthetic_span(),
    }
}

fn synthetic_span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}
