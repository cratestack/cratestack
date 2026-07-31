//! Read-only field list + edit-mode field rows rendered inside the
//! drawer.

use leptos::prelude::*;

use crate::editors::render_typed_input_optional;
use crate::types::{FieldError, FieldSummary, ModelSummary};

use super::format::{format_cell, format_value_html};

#[component]
pub fn FieldList(row: serde_json::Map<String, serde_json::Value>) -> impl IntoView {
    view! {
        <div class="overflow-hidden rounded-box border border-base-300">
            <table class="table table-zebra table-xs">
                <tbody>
                    {row.iter().map(|(k, v)| {
                        let key = k.clone();
                        let value = format_value_html(v);
                        view! {
                            <tr>
                                <td class="font-medium opacity-50 align-top w-1/3">{key}</td>
                                <td class="font-mono text-xs break-all whitespace-pre-wrap">{value}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

#[component]
pub fn EditFields(
    values: ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
    errors: ReadSignal<Vec<FieldError>>,
    set_values: WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
    model: ModelSummary,
) -> impl IntoView {
    let writable: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
        .cloned()
        .collect();
    view! {
        <dl class="text-sm space-y-2">
            {writable.into_iter().map(|f| {
                let name = f.name.clone();
                let name_for_error = name.clone();
                let field_for_input = f.clone();
                view! {
                    <div class="grid grid-cols-3 gap-2 items-start">
                        <dt class="text-base-content/50 pt-1">{name.clone()}</dt>
                        <dd class="col-span-2">
                            {render_typed_input_optional(field_for_input, values, set_values)}
                            {move || errors.get().iter()
                                .find(|e| e.field == name_for_error)
                                .map(|e| view! {
                                    <p class="text-xs text-error mt-0.5">{e.message.clone()}</p>
                                }.into_any())
                                .unwrap_or_else(|| ().into_any())}
                        </dd>
                    </div>
                }
            }).collect_view()}
        </dl>
    }
}

/// Pull the primary key out of a row, formatting whatever scalar shape
/// the backend returned as a string. Used by the drawer when building
/// snippet/follow requests that include `pk` on the URL.
pub fn row_pk(row: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    row.get("id").map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Snapshot a row's writable fields into the edit-mode signal map.
///
/// JSON `null` is mapped to `""`, not through [`format_cell`]: that
/// helper's `"—"` is a *display* placeholder for the read-only
/// table/drawer, and every editor widget (text, number, boolean and
/// enum `<select>`, see `editors::render`) already treats `""` as its
/// "no value" sentinel — `build_payload` turns an empty string back
/// into `Value::Null` for optional fields. Routing `null` through
/// `format_cell` here would seed the form with the literal `"—"`
/// string, so an untouched NULL field gets corrupted into that string
/// the moment the user clicks Save.
pub fn snapshot_for_edit(
    row: &serde_json::Map<String, serde_json::Value>,
    model: &ModelSummary,
) -> std::collections::BTreeMap<String, String> {
    model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
        .map(|f| {
            let v = match row.get(&f.name) {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(value) => format_cell(value),
            };
            (f.name.clone(), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FieldSummary;

    fn field(name: &str, type_name: &str, arity: &str) -> FieldSummary {
        FieldSummary {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            arity: arity.to_owned(),
            is_id: false,
            is_relation: false,
            is_enum: false,
            enum_variants: Vec::new(),
        }
    }

    fn model(fields: Vec<FieldSummary>) -> ModelSummary {
        ModelSummary {
            name: "Widget".to_owned(),
            primary_key: Some("id".to_owned()),
            fields,
        }
    }

    #[test]
    fn snapshot_maps_null_to_empty_string_not_display_placeholder() {
        let model = model(vec![field("nickname", "String", "optional")]);
        let mut row = serde_json::Map::new();
        row.insert("nickname".to_owned(), serde_json::Value::Null);

        let snapshot = snapshot_for_edit(&row, &model);

        assert_eq!(snapshot.get("nickname"), Some(&String::new()));
    }

    #[test]
    fn edit_then_save_with_no_changes_keeps_null_field_null() {
        // Reproduces the reported bug end-to-end: open a row with a
        // NULL optional column for edit, then Save without touching
        // anything. The saved payload must omit the field as Null,
        // never as the "—" display placeholder.
        let fields = vec![field("nickname", "String", "optional")];
        let model = model(fields.clone());
        let mut row = serde_json::Map::new();
        row.insert("nickname".to_owned(), serde_json::Value::Null);

        let snapshot = snapshot_for_edit(&row, &model);
        let payload = crate::editors::build_payload(&fields, &snapshot);

        assert_eq!(payload.get("nickname"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn snapshot_preserves_non_null_values_via_format_cell() {
        let model = model(vec![field("nickname", "String", "optional")]);
        let mut row = serde_json::Map::new();
        row.insert(
            "nickname".to_owned(),
            serde_json::Value::String("ada".to_owned()),
        );

        let snapshot = snapshot_for_edit(&row, &model);

        assert_eq!(snapshot.get("nickname"), Some(&"ada".to_owned()));
    }
}
