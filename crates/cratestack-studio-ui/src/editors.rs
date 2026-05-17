//! Typed input editors used by the create + edit forms.
//!
//! The two public entry points — [`render_typed_input`] and
//! [`render_typed_input_optional`] — differ only in signal shape
//! (`BTreeMap` vs `Option<BTreeMap>`). They funnel through a shared
//! dispatcher in [`render`] so the per-type widget table is defined
//! once.

mod payload;
mod render;

use leptos::prelude::*;

use crate::types::FieldSummary;

pub use payload::build_payload;

use render::{ValueIo, render_dispatch};

/// Render the right input control for `field` and bind it to a
/// per-field key inside `values`.
pub fn render_typed_input(
    field: FieldSummary,
    values: ReadSignal<std::collections::BTreeMap<String, String>>,
    set_values: WriteSignal<std::collections::BTreeMap<String, String>>,
) -> impl IntoView {
    let name_for_read = field.name.clone();
    let name_for_write = field.name.clone();
    let io = ValueIo {
        read: move || values.with(|m| m.get(&name_for_read).cloned().unwrap_or_default()),
        write: move |v: String| set_values.update(|m| {
            m.insert(name_for_write.clone(), v);
        }),
    };
    render_dispatch(field, io)
}

/// Same signature as [`render_typed_input`] but bound to an
/// `Option<BTreeMap>` source (the drawer's edit-mode signal).
pub fn render_typed_input_optional(
    field: FieldSummary,
    values: ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
    set_values: WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
) -> impl IntoView {
    let name_for_read = field.name.clone();
    let name_for_write = field.name.clone();
    let io = ValueIo {
        read: move || {
            values.with(|opt| {
                opt.as_ref()
                    .and_then(|m| m.get(&name_for_read).cloned())
                    .unwrap_or_default()
            })
        },
        write: move |v: String| {
            set_values.update(|opt| {
                if let Some(m) = opt.as_mut() {
                    m.insert(name_for_write.clone(), v);
                }
            });
        },
    };
    render_dispatch(field, io)
}
