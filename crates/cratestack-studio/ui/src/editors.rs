//! Phase 1d typed input editors used by the create + edit forms.
//!
//! Studio's record forms used to be one text box per field; Phase 1d
//! replaces them with the right HTML5 control for each declared
//! scalar:
//!
//! - `<select>` for enums (variants from the schema)
//! - `<textarea>` for `Json` (with a parse hint)
//! - `<input type="datetime-local">` for `DateTime`
//! - `<input type="number">` for `Int` / `Float` / `Decimal`
//! - `<select>` (true/false/null) for `Boolean`
//! - plain text for everything else (`String`, `Cuid`, `Uuid`, `Bytes`)

use leptos::prelude::*;

use crate::types::FieldSummary;

/// Render the right input control for `field` and bind it to a
/// per-field key inside `values`. Mirrors the original plain-input
/// API, just dispatching on type metadata before painting.
pub fn render_typed_input(
    field: FieldSummary,
    values: ReadSignal<std::collections::BTreeMap<String, String>>,
    set_values: WriteSignal<std::collections::BTreeMap<String, String>>,
) -> impl IntoView {
    let name = field.name.clone();
    let name_for_input = name.clone();
    let name_for_value = name.clone();

    let placeholder = format!("{} ({})", field.type_name, field.arity);
    let type_name = field.type_name.clone();
    let is_enum = field.is_enum;
    let variants = field.enum_variants.clone();
    let optional = field.arity == "optional";

    let common_class = "w-full border border-slate-300 rounded px-2 py-1 text-xs font-mono";

    if is_enum {
        return view! {
            <select
                class=common_class
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            >
                <option value="">{if optional { "—" } else { "Select…" }}</option>
                {variants.into_iter().map(|v| {
                    let value = v.clone();
                    let label = v.clone();
                    view! { <option value=value>{label}</option> }
                }).collect_view()}
            </select>
        }.into_any();
    }

    match type_name.as_str() {
        "Json" => view! {
            <textarea
                class=common_class
                rows="4"
                placeholder="{ … }"
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            ></textarea>
        }.into_any(),
        "DateTime" => view! {
            <input
                type="datetime-local"
                class=common_class
                step="1"
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            />
        }.into_any(),
        "Decimal" | "Float" => view! {
            <input
                type="number"
                class=common_class
                step="any"
                placeholder=placeholder
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            />
        }.into_any(),
        "Int" => view! {
            <input
                type="number"
                class=common_class
                step="1"
                placeholder=placeholder
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            />
        }.into_any(),
        "Boolean" => view! {
            <select
                class=common_class
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            >
                <option value="">{if optional { "—" } else { "Select…" }}</option>
                <option value="true">"true"</option>
                <option value="false">"false"</option>
            </select>
        }.into_any(),
        _ => view! {
            <input
                type="text"
                class=common_class
                placeholder=placeholder
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    set_values.update(|m| { m.insert(name_for_input.clone(), v); });
                }
                prop:value=move || values.with(|m| m.get(&name_for_value).cloned().unwrap_or_default())
            />
        }.into_any(),
    }
}

/// Same signature as `render_typed_input` but bound to an
/// `Option<BTreeMap>` source (the drawer's edit-mode signal). Falls
/// through to the same renderers; only the value plumbing differs.
pub fn render_typed_input_optional(
    field: FieldSummary,
    values: ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
    set_values: WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
) -> impl IntoView {
    let name = field.name.clone();
    let name_for_input = name.clone();
    let name_for_value = name.clone();
    let placeholder = format!("{} ({})", field.type_name, field.arity);
    let type_name = field.type_name.clone();
    let is_enum = field.is_enum;
    let variants = field.enum_variants.clone();
    let optional = field.arity == "optional";

    let common_class = "w-full border border-slate-300 rounded px-2 py-1 text-xs font-mono";

    let read_value = move || -> String {
        values.with(|opt| {
            opt.as_ref()
                .and_then(|m| m.get(&name_for_value).cloned())
                .unwrap_or_default()
        })
    };
    let write_value = move |v: String| {
        set_values.update(|opt| {
            if let Some(m) = opt.as_mut() {
                m.insert(name_for_input.clone(), v);
            }
        });
    };

    if is_enum {
        return view! {
            <select class=common_class
                on:change=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            >
                <option value="">{if optional { "—" } else { "Select…" }}</option>
                {variants.into_iter().map(|v| {
                    let value = v.clone(); let label = v.clone();
                    view! { <option value=value>{label}</option> }
                }).collect_view()}
            </select>
        }.into_any();
    }

    match type_name.as_str() {
        "Json" => view! {
            <textarea class=common_class rows="4" placeholder="{ … }"
                on:input=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            ></textarea>
        }.into_any(),
        "DateTime" => view! {
            <input type="datetime-local" class=common_class step="1"
                on:input=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            />
        }.into_any(),
        "Decimal" | "Float" => view! {
            <input type="number" class=common_class step="any" placeholder=placeholder
                on:input=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            />
        }.into_any(),
        "Int" => view! {
            <input type="number" class=common_class step="1" placeholder=placeholder
                on:input=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            />
        }.into_any(),
        "Boolean" => view! {
            <select class=common_class
                on:change=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            >
                <option value="">{if optional { "—" } else { "Select…" }}</option>
                <option value="true">"true"</option>
                <option value="false">"false"</option>
            </select>
        }.into_any(),
        _ => view! {
            <input type="text" class=common_class placeholder=placeholder
                on:input=move |ev| { write_value(event_target_value(&ev)); }
                prop:value=read_value
            />
        }.into_any(),
    }
}

/// Build a JSON payload from a `String`-typed form map, parsing each
/// value into the right JSON variant based on the field's declared
/// type. Phase 1d adds DateTime / Decimal handling (both pass through
/// as strings to the server) and tightens Boolean / Int / Float
/// parsing so the validator gets a typed value rather than a string
/// that happens to look like a number.
pub fn build_payload(
    writable: &[FieldSummary],
    values: &std::collections::BTreeMap<String, String>,
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for f in writable {
        let raw = values.get(&f.name).cloned().unwrap_or_default();
        if raw.is_empty() {
            if f.arity == "optional" {
                out.insert(f.name.clone(), serde_json::Value::Null);
            }
            continue;
        }
        let v = if f.is_enum {
            serde_json::Value::String(raw.clone())
        } else {
            match f.type_name.as_str() {
                "Int" => raw
                    .parse::<i64>()
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::String(raw.clone())),
                "Float" => raw
                    .parse::<f64>()
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::String(raw.clone())),
                "Decimal" => serde_json::Value::String(raw.clone()),
                "DateTime" => serde_json::Value::String(normalize_datetime(&raw)),
                "Boolean" => match raw.as_str() {
                    "true" | "1" | "yes" => serde_json::Value::Bool(true),
                    "false" | "0" | "no" => serde_json::Value::Bool(false),
                    _ => serde_json::Value::String(raw.clone()),
                },
                "Json" => {
                    serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw.clone()))
                }
                _ => serde_json::Value::String(raw.clone()),
            }
        };
        out.insert(f.name.clone(), v);
    }
    serde_json::Value::Object(out)
}

/// `<input type="datetime-local">` returns `YYYY-MM-DDTHH:MM` (and
/// `YYYY-MM-DDTHH:MM:SS` when `step` is set). Either way we tack a
/// `Z` on so the backend sees an unambiguous UTC instant rather than
/// a local-time string that callers later have to guess at.
fn normalize_datetime(raw: &str) -> String {
    if raw.ends_with('Z') || raw.contains('+') {
        return raw.to_owned();
    }
    if raw.len() == 16 {
        // No seconds — append zero seconds.
        return format!("{raw}:00Z");
    }
    format!("{raw}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_appends_zero_seconds_and_z() {
        assert_eq!(normalize_datetime("2024-02-03T04:05"), "2024-02-03T04:05:00Z");
        assert_eq!(normalize_datetime("2024-02-03T04:05:06"), "2024-02-03T04:05:06Z");
    }

    #[test]
    fn datetime_leaves_trailing_zone_alone() {
        assert_eq!(
            normalize_datetime("2024-02-03T04:05:06Z"),
            "2024-02-03T04:05:06Z"
        );
        assert_eq!(
            normalize_datetime("2024-02-03T04:05:06+01:00"),
            "2024-02-03T04:05:06+01:00"
        );
    }
}
