//! Known attributes written in a form the generators do not read.
//!
//! Generators recognise most attributes by their raw text, and many compare
//! it whole (`a.raw == "@readonly"`), while `super::misspelled_attributes`
//! compares only the bare name. So a known name with an argument list it
//! does not take, or two attributes with no space between them, used to
//! report `schema OK` and do nothing. Two rules, on every field of every
//! field-bearing block (`model`, `mixin`, `type`, `view`, `auth`) and on the
//! block-level `@@` attributes of models and views:
//!
//! 1. **Run-on attributes.** An `@` outside any string literal and argument
//!    group, after the attribute's own leading `@`s, starts a second
//!    attribute that nothing reads: `@server_only@unique` (fields split only
//!    at whitespace), or `@@audit@@paged` / `@@audit @@paged` (a block-level
//!    attribute is its whole line). See [`scan`]. A trailing `//` comment
//!    is never part of an attribute: the parser strips it first.
//! 2. **A no-argument attribute followed by anything but the end of its
//!    text** — an argument list (`@readonly()`) or stray punctuation
//!    (`@readonly,`). A following letter, digit or `_` makes another name
//!    (`@unique_per_tenant`), which is left to `super::misspelled_attributes`.
//!
//! # Field attributes that take no arguments
//!
//! Derived from every reader of each name (paths under `crates/`). *Exact*
//! readers compare the raw text whole, so any other spelling has no effect
//! there.
//!
//! | Attribute      | Readers | Other spellings, before |
//! |----------------|---------|-------------------------|
//! | `@server_only` | exact: `cratestack-macros/src/shared/attrs.rs:53`, `cratestack-client-typescript/src/types.rs:140`, `…/wire_shapes.rs:172`, `cratestack-mock-wiremock/src/model_attrs.rs:31`, `cratestack-cli/src/schema_diff/fields.rs:116` | no effect |
//! | `@readonly`    | exact: `cratestack-macros/src/shared/attrs.rs:45` | no effect |
//! | `@version`     | exact: `cratestack-macros/src/shared/attrs.rs:100`, `…/model/descriptor.rs:165`, `…/axum/model/prep.rs:80`, `cratestack-client-typescript/src/types.rs:210`, `cratestack-mock-wiremock/src/model_attrs.rs:53`, `cratestack-studio/src/api/records/guards.rs:84` | no effect |
//! | `@pii`         | exact: `cratestack-macros/src/shared/attrs.rs:61` | no effect |
//! | `@sensitive`   | exact: `cratestack-macros/src/shared/attrs.rs:69` | no effect |
//! | `@db_enforce`  | exact: `cratestack-migrate/src/convert/checks.rs:12` | no effect |
//! | `@email`       | no-argument arm `cratestack-macros/src/validators.rs:78`; exact `cratestack-studio/src/validators/predicates.rs:20` | no effect (arguments already refused on model fields, `super::validators`) |
//! | `@uri`         | `cratestack-macros/src/validators.rs:79`; `cratestack-studio/src/validators/predicates.rs:23` | same |
//! | `@iso4217`     | `cratestack-macros/src/validators.rs:80`; `cratestack-studio/src/validators/predicates.rs:26`; `cratestack-migrate/src/convert/checks.rs:31` | same |
//! | `@unique`      | its only reader, `cratestack-migrate/src/convert/fields.rs:109`, takes `@unique` or `@unique(`…, never reading inside | arguments ignored; other punctuation no effect |
//! | `@id`          | exact, through the one shared matcher `cratestack_core::is_primary_key_attribute` (cratestack#1074): every generator, migrate and studio | read as `@id` on a model before cratestack#1074 (a prefix match), no effect on a view; arguments never read |
//!
//! Attributes that take arguments (`@default`, `@relation`, `@computed`,
//! `@from`, `@rename`, `@length`, `@range`, `@regex`) are left to their own
//! validators; `@rename`'s runs at the end of this one
//! (`super::rename_attributes`), after rule 1.
//!
//! # Block-level attributes that take no arguments
//!
//! | Attribute        | Readers | Other spellings, before |
//! |------------------|---------|-------------------------|
//! | `@@materialized` | `cratestack-core/src/schema/view.rs:73` via `has_bare_attribute` (`:114`): the name or `name(`… | arguments ignored; other punctuation no effect |
//! | `@@no_unique`    | `cratestack-core/src/schema/view.rs:79`, same | same |
//! | `@@audit`        | exact: `cratestack-macros/src/model/descriptor.rs:83` | arguments were already refused; other punctuation no effect |
//! | `@@soft_delete`  | exact: `cratestack-macros/src/model/descriptor.rs:87` | same |
//! | `@@subscribe`    | exact: `cratestack-macros/src/transport/subscribe_dispatch.rs:28`, `…/op_descriptors.rs:101` | same |
//!
//! `@@paged` is not listed: `super::model_attributes` already refuses every
//! spelling of it but the exact one.

mod block;
pub(super) mod scan;

pub(super) use block::validate_block_attribute_spelling;

use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

/// How the readers of a no-argument field attribute match its raw text.
#[derive(Clone, Copy)]
enum Readers {
    /// Every reader compares the text whole.
    Exact,
    /// Its one reader also takes `name(`…, and never reads inside.
    AcceptsArguments,
}

/// See the module doc's first table for where each entry comes from.
const NO_ARGUMENT_FIELD_ATTRIBUTES: &[(&str, Readers)] = &[
    ("@server_only", Readers::Exact),
    ("@readonly", Readers::Exact),
    ("@version", Readers::Exact),
    ("@pii", Readers::Exact),
    ("@sensitive", Readers::Exact),
    ("@db_enforce", Readers::Exact),
    ("@email", Readers::Exact),
    ("@uri", Readers::Exact),
    ("@iso4217", Readers::Exact),
    ("@unique", Readers::AcceptsArguments),
    // Exact since cratestack#1074; this is the one check refusing `@id(...)`.
    ("@id", Readers::Exact),
];

/// The module doc's second table, per block kind.
pub(super) const NO_ARGUMENT_VIEW_ATTRIBUTES: &[&str] = &["@@materialized", "@@no_unique"];
pub(super) const NO_ARGUMENT_MODEL_ATTRIBUTES: &[&str] =
    &["@@audit", "@@soft_delete", "@@subscribe"];

/// The text after `name` in `raw`, when `raw` is `name` followed by
/// something that does not continue the identifier.
fn trailing_text<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.strip_prefix(name)
        .filter(|rest| !rest.is_empty())
        .filter(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// Rules 1 and 2 for one field of any block.
pub(super) fn validate_field_attribute_spelling(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let owner = format!("field `{}` on {owner_kind} `{owner_name}`", field.name);
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        let offsets = scan::run_on_offsets(raw);
        if !offsets.is_empty() {
            return Err(span_error(
                format!(
                    "{owner} writes `{raw}`: attributes with no space between them are read \
                     as one unrecognised attribute, so this is refused. Separate them with a \
                     space: `{}`",
                    scan::separated(raw, &offsets, " "),
                ),
                attribute.span,
            ));
        }
        let Some((name, readers, rest)) = NO_ARGUMENT_FIELD_ATTRIBUTES
            .iter()
            .find_map(|(name, readers)| Some((*name, *readers, trailing_text(raw, name)?)))
        else {
            continue;
        };
        // An argument list: cratestack#1074's wording for `@id(...)`, for
        // every name in the table.
        let message = if rest.starts_with('(') {
            let why = match readers {
                Readers::Exact => format!(
                    "every generator recognises it only when written exactly `{name}`, so this \
                     spelling has no effect"
                ),
                Readers::AcceptsArguments => "no generator reads the argument list".to_owned(),
            };
            format!(
                "{owner} writes `{raw}`, but `{name}` takes no arguments — write `{name}`: {why}"
            )
        } else {
            format!(
                "{owner} writes `{raw}`; `{name}` is recognised only when written exactly \
                 `{name}`, so this spelling has no effect. It is refused: write `{name}`, with a \
                 space before any following attribute",
            )
        };
        return Err(span_error(message, attribute.span));
    }
    super::rename_attributes::validate_field_rename(owner_kind, owner_name, field)
}
