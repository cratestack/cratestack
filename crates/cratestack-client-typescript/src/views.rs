use std::collections::{BTreeSet, HashMap};

use cratestack_core::route_naming;
use cratestack_core::{EnumDecl, Field, Model, TypeArity};
use serde::Serialize;

use crate::naming::{escape_ts_string, pluralize, to_camel_case, to_kebab_case, ts_identifier};
use crate::types::{is_paged_model, model_allows_create, primary_key_field, ts_type};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) union: String,
    pub(crate) values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InterfaceView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) property: String,
    pub(crate) wire_name: String,
    pub(crate) type_name: String,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelApiView {
    pub(crate) name: String,
    pub(crate) api_name: String,
    pub(crate) accessor: String,
    /// Issue #610: `README.md.j2`'s `--swr` section needs this model's
    /// `swr` per-model file path (`{{ package_name }}/swr/models/{{
    /// file_stem }}`) for its optimistic-concurrency example — the same
    /// `to_kebab_case(&model.name)` `crate::swr::context` uses to name
    /// that file, kept in sync by sharing the helper rather than
    /// duplicating the transform.
    pub(crate) file_stem: String,
    pub(crate) route: String,
    pub(crate) primary_key_type: String,
    pub(crate) allows_create: bool,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) list_return_type: String,
    /// `true` when `list_return_type` is `Page<{Model}>` rather than
    /// `{Model}[]` — the generated `list()` method needs to know this to
    /// pick `revivePagedDecimalFields` (applies this model's decimal shape
    /// to the envelope's `.items`) over plain `reviveDecimalFields`
    /// (cratestack#499: a `Page<T>` envelope's own keys — `items`/
    /// `totalCount`/`pageInfo` — are never themselves `T`'s fields, so
    /// `T`'s shape can't be applied to the envelope directly).
    pub(crate) is_paged: bool,
    /// This model's own registry key into the generated `decimalShapes`
    /// object (`models.ts.j2`) — always just `name` (a model's shape is
    /// always registered under its own schema name), spliced verbatim into
    /// `rest-client.ts.j2`/`rpc-client.ts.j2`'s `reviveDecimalFields(value,
    /// '{{ decimal_shape_name }}')` call at every CRUD method that decodes
    /// a server response. See `crate::decimal`'s module doc for why this
    /// replaced a flat, name-keyed `decimalKeys: string[]` (cratestack#499
    /// review: that scheme had a reachable field-name-collision hazard).
    pub(crate) decimal_shape_name: String,
    pub(crate) list_query_key: String,
    pub(crate) get_query_key: String,
    pub(crate) create_mutation_key: String,
    pub(crate) update_mutation_key: String,
    pub(crate) delete_mutation_key: String,
}

#[derive(Clone, Copy)]
pub(crate) enum InterfaceKind {
    Plain,
    Patch,
    Model,
}

pub(crate) fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    let values = enum_decl
        .variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    let union = values
        .iter()
        .map(|value| format!("'{}'", escape_ts_string(value)))
        .collect::<Vec<_>>()
        .join(" | ");
    EnumView {
        name: enum_decl.name.clone(),
        union,
        values,
    }
}

pub(crate) fn build_interface(
    name: &str,
    fields: &[&Field],
    kind: InterfaceKind,
    enum_names: &BTreeSet<&str>,
) -> InterfaceView {
    InterfaceView {
        name: name.to_owned(),
        has_fields: !fields.is_empty(),
        fields: fields
            .iter()
            .map(|field| {
                let optional = match kind {
                    InterfaceKind::Patch | InterfaceKind::Model => true,
                    InterfaceKind::Plain => field.ty.arity == TypeArity::Optional,
                };
                FieldView {
                    property: ts_identifier(&field.name),
                    wire_name: field.name.clone(),
                    type_name: ts_type(&field.ty, enum_names),
                    optional,
                }
            })
            .collect(),
    }
}

pub(crate) fn build_model_api(model: &Model) -> ModelApiView {
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    // cratestack#345: this route must match the server's real Axum route
    // registration exactly (`cratestack-macros::axum::model::routes`), so
    // it's derived through the shared canonical algorithm rather than
    // this crate's own `to_snake_case`/`pluralize` (which exist for
    // client-only identifier naming — accessor/hook/method names below —
    // and are not wire-format contracts).
    let route = format!("/{}", route_naming::model_route_segment(&model.name));
    let accessor = pluralize(&to_camel_case(&model.name));
    let is_paged = is_paged_model(model);
    ModelApiView {
        name: model.name.clone(),
        api_name: format!("{}Api", model.name),
        accessor,
        file_stem: to_kebab_case(&model.name),
        route,
        primary_key_type: ts_type(&primary_key.ty, &BTreeSet::new()),
        allows_create: model_allows_create(model),
        create_input_name: format!("Create{}Input", model.name),
        update_input_name: format!("Update{}Input", model.name),
        list_return_type: if is_paged {
            format!("Page<{}>", model.name)
        } else {
            format!("{}[]", model.name)
        },
        is_paged,
        decimal_shape_name: model.name.clone(),
        list_query_key: format!("{}List", to_camel_case(&model.name)),
        get_query_key: format!("{}Detail", to_camel_case(&model.name)),
        create_mutation_key: format!("{}Create", to_camel_case(&model.name)),
        update_mutation_key: format!("{}Update", to_camel_case(&model.name)),
        delete_mutation_key: format!("{}Delete", to_camel_case(&model.name)),
    }
}

/// `list_query_key`/`get_query_key`/etc. are each derived from
/// `to_camel_case(&model.name)`, which is a lossy transform: two
/// distinct, parser-guaranteed-unique model names (e.g. `UserGroup` and
/// `User_Group`) can normalize to the same camelCase prefix and
/// therefore the same key. `list_query_key`/`get_query_key` are rendered
/// as sibling property names in the same `cratestackQueryKeys` object
/// literal (`rest-react-query.ts.j2`/`rpc-react-query.ts.j2`), so an
/// undetected collision is a TypeScript compile error
/// (`ts(1117)`), not just a runtime cache-key overlap.
///
/// Call this once per schema, after every model's `ModelApiView` has
/// been built, so each field's collisions can be detected across the
/// *whole* model list rather than per-model in isolation. Colliding
/// entries are suffixed with their own model's raw name — which the
/// parser already guarantees is unique verbatim across the schema
/// (`cratestack-parser`'s `ensure_unique` over the shared type/model/enum
/// namespace) — so the disambiguated key is guaranteed unique too.
pub(crate) fn disambiguate_model_api_keys(models: &mut [ModelApiView]) {
    disambiguate_field(
        models,
        |view| &view.list_query_key,
        |view, key| {
            view.list_query_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.get_query_key,
        |view, key| {
            view.get_query_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.create_mutation_key,
        |view, key| {
            view.create_mutation_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.update_mutation_key,
        |view, key| {
            view.update_mutation_key = key;
        },
    );
    disambiguate_field(
        models,
        |view| &view.delete_mutation_key,
        |view, key| {
            view.delete_mutation_key = key;
        },
    );
}

fn disambiguate_field(
    models: &mut [ModelApiView],
    get: impl Fn(&ModelApiView) -> &String,
    mut set: impl FnMut(&mut ModelApiView, String),
) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for view in models.iter() {
        *counts.entry(get(view).clone()).or_insert(0) += 1;
    }
    for view in models.iter_mut() {
        if counts.get(get(view).as_str()).copied().unwrap_or(0) > 1 {
            let disambiguated = format!("{}_{}", get(view), view.name);
            set(view, disambiguated);
        }
    }
}

/// Renders `items` as a TS array-literal source fragment of single-quoted
/// string literals (`['a', 'b']`, or `[]` for an empty slice) — the same
/// quoting `build_enum_view`'s union-type rendering uses, via the same
/// `escape_ts_string` helper. `pub(crate)` (not just used within this
/// module) since `crate::decimal::build_shape` reuses it for each
/// `DecimalShapeView`'s `keys_js`.
pub(crate) fn js_string_array(items: &[String]) -> String {
    let quoted = items
        .iter()
        .map(|item| format!("'{}'", escape_ts_string(item)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{quoted}]")
}
