//! The decode registry for ordinal enums (`Pending = 1`,
//! `docs/design/enum-ordinals.md` §2.2): a response carries the number,
//! and the generated client hands application code the variant's name, so
//! the app-facing type stays `'Pending' | 'Approved'`.
//!
//! Two tables, rendered only when the schema has an ordinal enum so every
//! other schema's output is byte-identical: each ordinal enum's
//! number → name map, and per model/`type` shape, which fields hold one.
//!
//! Requests still send names: the TypeScript encoder has no per-type
//! request shapes to hook, and the server decodes either form (§2.3).

use cratestack_core::{Field, Schema};
use serde::Serialize;

use crate::naming::escape_ts_string;

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct OrdinalRegistry {
    pub(crate) enums: Vec<OrdinalEnumView>,
    pub(crate) fields: Vec<OrdinalFieldsView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OrdinalEnumView {
    pub(crate) name: String,
    /// `{ 1: 'Pending', 2: 'Approved' }`.
    pub(crate) names_js: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OrdinalFieldsView {
    pub(crate) shape: String,
    /// `{ 'status': 'Status' }`.
    pub(crate) fields_js: String,
}

pub(crate) fn build_ordinal_registry(schema: &Schema) -> OrdinalRegistry {
    let enums: Vec<OrdinalEnumView> = schema
        .enums
        .iter()
        .filter(|e| e.is_ordinal())
        .map(|e| {
            let body = e
                .variants
                .iter()
                .filter_map(|v| Some(format!("{}: '{}'", v.ordinal?, escape_ts_string(&v.name))))
                .collect::<Vec<_>>()
                .join(", ");
            OrdinalEnumView {
                name: e.name.clone(),
                names_js: format!("{{ {body} }}"),
            }
        })
        .collect();
    if enums.is_empty() {
        return OrdinalRegistry::default();
    }
    let is_ordinal = |name: &str| enums.iter().any(|e| e.name == name);
    let shapes = schema
        .models
        .iter()
        .map(|m| (&m.name, &m.fields))
        .chain(schema.types.iter().map(|t| (&t.name, &t.fields)));
    let fields = shapes
        .filter_map(|(shape, fields)| {
            let entries: Vec<String> = fields
                .iter()
                .filter(|f| is_ordinal(&f.ty.name) && !is_server_only(f))
                .map(|f| {
                    format!(
                        "'{}': '{}'",
                        escape_ts_string(&f.name),
                        escape_ts_string(&f.ty.name)
                    )
                })
                .collect();
            (!entries.is_empty()).then(|| OrdinalFieldsView {
                shape: shape.clone(),
                fields_js: format!("{{ {} }}", entries.join(", ")),
            })
        })
        .collect();
    OrdinalRegistry { enums, fields }
}

/// `@server_only` fields never reach a response, as in `wire_shapes`.
fn is_server_only(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}
