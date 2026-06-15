//! Type-dispatch for the editor input widgets.
//!
//! `render_typed_input` and `render_typed_input_optional` used to be
//! near-identical 90-line dispatchers — the only difference was the
//! signal shape (`BTreeMap` vs `Option<BTreeMap>`). Both now funnel
//! through [`render_dispatch`], which takes a getter/setter pair so
//! the dispatch table is written once.

use leptos::prelude::*;

use crate::types::FieldSummary;

const INPUT_CLASS: &str = "input input-bordered input-sm w-full font-mono text-xs";
const TEXTAREA_CLASS: &str = "textarea textarea-bordered textarea-sm w-full font-mono text-xs";
const SELECT_CLASS: &str = "select select-bordered select-sm w-full font-mono text-xs";

/// Read/write accessor pair for a single field's text value. Both
/// closures are `Copy` because Leptos signal handles are `Copy`.
pub(super) struct ValueIo<R, W>
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    pub read: R,
    pub write: W,
}

pub(super) fn render_dispatch<R, W>(field: FieldSummary, io: ValueIo<R, W>) -> AnyView
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    let placeholder = format!("{} ({})", field.type_name, field.arity);
    let optional = field.arity == "optional";

    if field.is_enum {
        return render_enum(io, field.enum_variants, optional);
    }
    match field.type_name.as_str() {
        "Json" => render_textarea(io),
        "DateTime" => render_input(io, "datetime-local", "1", String::new()),
        "Decimal" | "Float" => render_input(io, "number", "any", placeholder),
        "Int" => render_input(io, "number", "1", placeholder),
        "Boolean" => render_boolean(io, optional),
        _ => render_input(io, "text", "", placeholder),
    }
}

fn render_input<R, W>(io: ValueIo<R, W>, ty: &'static str, step: &'static str, placeholder: String) -> AnyView
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    let ValueIo { read, write } = io;
    view! {
        <input
            type=ty
            class=INPUT_CLASS
            step=step
            placeholder=placeholder
            on:input=move |ev| { write(event_target_value(&ev)); }
            prop:value=read
        />
    }
    .into_any()
}

fn render_textarea<R, W>(io: ValueIo<R, W>) -> AnyView
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    let ValueIo { read, write } = io;
    view! {
        <textarea
            class=TEXTAREA_CLASS
            rows="4"
            placeholder="{ … }"
            on:input=move |ev| { write(event_target_value(&ev)); }
            prop:value=read
        ></textarea>
    }
    .into_any()
}

fn render_boolean<R, W>(io: ValueIo<R, W>, optional: bool) -> AnyView
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    let ValueIo { read, write } = io;
    let placeholder = if optional { "—" } else { "Select…" };
    view! {
        <select class=SELECT_CLASS
            on:change=move |ev| { write(event_target_value(&ev)); }
            prop:value=read
        >
            <option value="">{placeholder}</option>
            <option value="true">"true"</option>
            <option value="false">"false"</option>
        </select>
    }
    .into_any()
}

fn render_enum<R, W>(io: ValueIo<R, W>, variants: Vec<String>, optional: bool) -> AnyView
where
    R: Fn() -> String + Send + Sync + 'static,
    W: Fn(String) + Send + Sync + 'static,
{
    let ValueIo { read, write } = io;
    let placeholder = if optional { "—" } else { "Select…" };
    view! {
        <select class=SELECT_CLASS
            on:change=move |ev| { write(event_target_value(&ev)); }
            prop:value=read
        >
            <option value="">{placeholder}</option>
            {variants.into_iter().map(|v| {
                let value = v.clone();
                let label = v.clone();
                view! { <option value=value>{label}</option> }
            }).collect_view()}
        </select>
    }
    .into_any()
}
