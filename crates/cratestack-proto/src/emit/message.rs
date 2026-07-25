//! Renders a `message { ... }` block from a field list and the numbers
//! `build_lock` already assigned it.

use std::collections::BTreeMap;

use cratestack_core::Field;

use super::error::ProtoEmitError;
use super::field::render_field;
use super::scalar::map_scalar;

pub(super) struct RenderedMessage {
    pub(super) text: String,
    pub(super) needs_timestamp_import: bool,
}

pub(super) fn render_message(
    name: &str,
    fields: &[&Field],
    numbers: &BTreeMap<String, i32>,
) -> Result<RenderedMessage, ProtoEmitError> {
    // `PageInfo` mirrors `cratestack_core::page::PageInfo` exactly: its two
    // `bool` fields are never absent (computed on every response, never
    // subject to CrateStack's partial-selection semantics that motivate
    // the universal-optional rule elsewhere), so they deliberately opt out
    // of it rather than going through the generic renderer below. See
    // docs/design/protobuf.md §4.4 note carried into ticket #169's Part C.
    if name == "PageInfo" {
        return render_page_info(name, fields, numbers);
    }

    let mut text = format!("message {name} {{\n");
    let mut needs_timestamp_import = false;
    for field in fields {
        let number = lookup(numbers, name, &field.name)?;
        // `<Model>RpcListInput.include_fields` (`rpc_input_synth.rs`) is
        // the one field this crate emits that proto3's `map<K, V>` syntax
        // is needed for — not expressible via an ordinary `TypeRef`/arity
        // pair, so it bypasses `render_field` the same way `PageInfo`'s
        // bool fields bypass it below. Maps can't be `optional` any more
        // than `repeated` can (proto3 forbids both), so no presence
        // keyword either.
        if name.ends_with("RpcListInput") && field.name == "include_fields" {
            text.push_str(&format!(
                "  map<string, StringList> include_fields = {number};\n"
            ));
            continue;
        }
        let rendered = render_field(&field.name, &field.ty, number);
        needs_timestamp_import |= rendered.needs_timestamp_import;
        text.push_str("  ");
        text.push_str(&rendered.line);
        text.push('\n');
    }
    text.push_str("}\n");
    Ok(RenderedMessage {
        text,
        needs_timestamp_import,
    })
}

fn render_page_info(
    name: &str,
    fields: &[&Field],
    numbers: &BTreeMap<String, i32>,
) -> Result<RenderedMessage, ProtoEmitError> {
    let mut text = format!("message {name} {{\n");
    for field in fields {
        let number = lookup(numbers, name, &field.name)?;
        let mapped = map_scalar(&field.ty.name);
        let presence = match field.name.as_str() {
            "has_next_page" | "has_previous_page" => "",
            _ => "optional ",
        };
        text.push_str(&format!(
            "  {presence}{} {} = {number};\n",
            mapped.proto_type, field.name
        ));
    }
    text.push_str("}\n");
    Ok(RenderedMessage {
        text,
        needs_timestamp_import: false,
    })
}

fn lookup(
    numbers: &BTreeMap<String, i32>,
    owner: &str,
    field: &str,
) -> Result<i32, ProtoEmitError> {
    numbers
        .get(field)
        .copied()
        .ok_or_else(|| ProtoEmitError::MissingLockEntry {
            owner: owner.to_owned(),
            field: field.to_owned(),
        })
}
