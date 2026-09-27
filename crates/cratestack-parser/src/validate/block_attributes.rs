//! The closed list of `@@` block attributes, per block kind
//! (GHSA-69g4-xvcm-vm2j, maintainer decision 2).
//!
//! Every reader matches a block attribute by its text, so a name no reader
//! knows had no effect at all — and `@@alow("read", …)`, two typos from
//! `@@allow`, or a Prisma habit such as `@@map("docs")`, passed `cratestack
//! check` and did nothing. Every `@@` attribute of a model or view must now
//! be one of the names below, spelled exactly, with an argument list
//! exactly when the name takes one, and nothing after it
//! (`super::attribute_shape::check_shape`, shared with procedures and
//! queries). Runs after the per-attribute validators, so their more
//! specific messages still win where they apply.
//!
//! Only models and views take `@@` attributes. A `query` has its own closed
//! list (`super::query_attributes`); a `mixin`, `type` or `auth` body is
//! fields only, so a `@@` line there is refused as a field ("expected field
//! type"); an `enum` body is variants only.
//!
//! | Block | Attribute | Arguments | Readers (paths under `crates/`) |
//! |-------|-----------|-----------|----------------------------------|
//! | model, view | `@@allow` | required | `cratestack-macros/src/policy/model.rs:68`; models also `cratestack-macros/src/axum/policy_attr.rs:31`, `cratestack-client-typescript/src/types.rs:156` |
//! | model, view | `@@deny` | required | `cratestack-macros/src/policy/model.rs:90` |
//! | model | `@@emit` | required | `cratestack-macros/src/event.rs:9`, `cratestack-core/src/events.rs:89`, `cratestack-studio/src/data/model_info.rs:121` |
//! | model | `@@paged` | none | `cratestack-macros/src/shared/attrs.rs:37` (and the Dart/TS/wiremock generators) |
//! | model | `@@audit` | none | `cratestack-macros/src/model/descriptor.rs:83` |
//! | model | `@@soft_delete` | none | `cratestack-macros/src/model/descriptor.rs:87` |
//! | model | `@@retain` | required | `cratestack-macros/src/model/descriptor.rs:99` |
//! | model | `@@subscribe` | none | `cratestack-macros/src/transport/subscribe_dispatch.rs:28` |
//! | model | `@@id` | required | `cratestack-core/src/composite_id.rs:36`, `cratestack-migrate/src/convert.rs:101` |
//! | model | `@@unique` | required (its own validator refuses a bare one first, with a more specific message) | `cratestack-core/src/schema/composite_unique.rs:47`, `cratestack-migrate/src/convert/uniques.rs:35` |
//! | model | `@@index` | required (same) | `cratestack-core/src/schema/index_attribute.rs:66`, `cratestack-migrate/src/convert/indexes.rs:36` |
//! | model | `@@internal` | required | `cratestack-core/src/schema/internal_attribute.rs:138` |
//! | model | `@@rename` | required, exactly `from = "<old_table>"`, at most once (`super::rename_attributes`) | `cratestack-migrate/src/convert/renames.rs:12` (migrations only), through `cratestack-core/src/schema/rename_attribute.rs` |
//! | view | `@@server_sql` | required | `cratestack-core/src/schema/view.rs:56` |
//! | view | `@@embedded_sql` | required | `cratestack-core/src/schema/view.rs:65` |
//! | view | `@@sql` | required | `cratestack-core/src/schema/view.rs:57` |
//! | view | `@@materialized` | none | `cratestack-core/src/schema/view.rs:73` |
//! | view | `@@no_unique` | none | `cratestack-core/src/schema/view.rs:79` |
//!
//! `@@rename`'s arguments are checked too, with the reader
//! `cratestack-migrate` uses (`cratestack_core::schema::parse_rename_from`):
//! any other argument text was read as no marker at all, so the next
//! migration dropped the old table and created the new one instead of
//! renaming it. A field's `@rename` gets the same check, and a second
//! marker of either kind is refused, in `super::rename_attributes`.
//!
//! A model's `@@mcp(resource: …)` is not listed: the parser moves it into
//! `Model::mcp` (`crate::parse::mcp::attribute::extract_model_mcp`) before
//! validation, and `super::mcp` refuses it on a view. A model's `@use(…)`
//! is expanded into fields while parsing (`crate::parse::models`).

use cratestack_core::Attribute;
use cratestack_core::schema::{RENAME_ARGUMENT_FORM, parse_rename_from};

use super::attribute_shape::{Arguments, Known, check_shape};
use crate::diagnostics::{SchemaError, span_error};

const MODEL_BLOCK_ATTRIBUTES: &[Known] = &[
    ("@@allow", Arguments::Required),
    ("@@deny", Arguments::Required),
    ("@@emit", Arguments::Required),
    ("@@paged", Arguments::None),
    ("@@audit", Arguments::None),
    ("@@soft_delete", Arguments::None),
    ("@@retain", Arguments::Required),
    ("@@subscribe", Arguments::None),
    ("@@id", Arguments::Required),
    ("@@unique", Arguments::Required),
    ("@@index", Arguments::Required),
    ("@@internal", Arguments::Required),
    ("@@rename", Arguments::Required),
];

const VIEW_BLOCK_ATTRIBUTES: &[Known] = &[
    ("@@allow", Arguments::Required),
    ("@@deny", Arguments::Required),
    ("@@server_sql", Arguments::Required),
    ("@@embedded_sql", Arguments::Required),
    ("@@sql", Arguments::Required),
    ("@@materialized", Arguments::None),
    ("@@no_unique", Arguments::None),
];

/// Checks one `@@` attribute of the model `name` against the model list.
pub(super) fn validate_model_block_attribute(
    name: &str,
    attribute: &Attribute,
) -> Result<(), SchemaError> {
    let owner = format!("model `{name}`");
    match check_shape(attribute, MODEL_BLOCK_ATTRIBUTES, &owner, "model")? {
        ("@@rename", Some(arguments)) if parse_rename_from(arguments).is_none() => Err(span_error(
            format!(
                "{owner} writes `{}`: `@@rename` takes exactly one argument, \
                     `@@rename({RENAME_ARGUMENT_FORM})` — the SQL table name being renamed, \
                     for example `@@rename(from = \"documents\")`. `cratestack migrate` reads \
                     no other form, and an unread marker makes the next migration drop the \
                     old table and create a new one, losing its rows. It is refused",
                attribute.raw
            ),
            attribute.span,
        )),
        _ => Ok(()),
    }
}

/// Checks one `@@` attribute of the view `name` against the view list.
pub(super) fn validate_view_block_attribute(
    name: &str,
    attribute: &Attribute,
) -> Result<(), SchemaError> {
    let owner = format!("view `{name}`");
    check_shape(attribute, VIEW_BLOCK_ATTRIBUTES, &owner, "view").map(|_| ())
}

#[cfg(test)]
mod tests {
    use cratestack_core::{Attribute, SourceSpan};

    use super::{validate_model_block_attribute, validate_view_block_attribute};

    fn attribute(raw: &str) -> Attribute {
        Attribute {
            raw: raw.to_owned(),
            span: SourceSpan {
                start: 0,
                end: raw.len(),
                line: 1,
            },
        }
    }

    /// Every reader of `@@unique` / `@@index` matches `@@unique(` /
    /// `@@index(`, so the list itself refuses the bare name rather than
    /// relying on the per-attribute validator that runs before it.
    #[test]
    fn the_list_itself_refuses_an_attribute_no_reader_reads_bare() {
        for raw in [
            "@@unique", "@@index", "@@id", "@@emit", "@@rename", "@@allow",
        ] {
            let error = validate_model_block_attribute("Doc", &attribute(raw))
                .expect_err(raw)
                .to_string();
            assert!(error.contains("takes an argument list"), "{raw}: {error}");
        }
        for raw in ["@@server_sql", "@@sql", "@@deny"] {
            validate_view_block_attribute("V", &attribute(raw)).expect_err(raw);
        }
    }
}
