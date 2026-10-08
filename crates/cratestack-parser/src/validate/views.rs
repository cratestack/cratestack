//! `view` block semantic checks (ADR-0003).
//!
//! Each rule is enforced independently and produces a span-pointed
//! `SchemaError`. Rules:
//!
//! 1. View names are unique within the schema.
//! 2. Every `from <Model>` source resolves to an existing model.
//! 3. At least one of `@@server_sql` / `@@embedded_sql` / `@@sql` must
//!    be present — a view with no SQL body is meaningless.
//! 4. `@@materialized` is server-only at the schema level too: it
//!    requires `@@server_sql` (or `@@sql`).
//! 5. `@@materialized` is incompatible with `@@no_unique` — concurrent
//!    refresh requires a unique index.
//! 6. Exactly one field must carry `@id` unless `@@no_unique` is set.
//! 7. `@@allow` is supported only with action `"read"`.

use std::collections::BTreeSet;

use cratestack_core::{Schema, TypeArity, View};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::attribute_spelling::{
    NO_ARGUMENT_VIEW_ATTRIBUTES, validate_block_attribute_spelling,
};
use crate::validate::block_attributes::validate_view_block_attribute;
use crate::validate::builder_setter_collisions::{
    validate_no_add_setter_collision, validate_no_build_setter_collision,
};
use crate::validate::collect::record;
use crate::validate::computed_attribute::{
    ComputedFieldSupport, validate_computed_field_attribute,
};
use crate::validate::field_attributes::{FieldHost, validate_field_attributes};
use crate::validate::fields::validate_field_reserved_identifier;
use crate::validate::key_relation_attributes::validate_key_and_relation_attributes;
use crate::validate::policy_attributes::{
    MODEL_ACTIONS, VIEW_DENY_ACTIONS, validate_policy_attribute,
};
use crate::validate::reserved_idents::validate_reserved_identifier;
use crate::validate::snake_case_collisions::validate_field_column_collisions;

/// Each view is checked independently so one bad view does not hide the next.
pub(super) fn validate_views_collecting(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let model_names: BTreeSet<&str> = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect();

    let mut seen = BTreeSet::new();
    for view in &schema.views {
        record(errors, || {
            if !seen.insert(view.name.clone()) {
                return Err(span_error(
                    format!("duplicate view name `{}`", view.name),
                    view.span,
                ));
            }
            validate_view(view, &model_names)
        });
    }
}

fn validate_view(view: &View, model_names: &BTreeSet<&str>) -> Result<(), SchemaError> {
    validate_reserved_identifier(&view.name, view.name_span, &format!("view `{}`", view.name))?;
    validate_field_column_collisions(&view.fields, "view", &view.name)?;
    for attribute in &view.attributes {
        validate_block_attribute_spelling(
            "view",
            &view.name,
            attribute,
            NO_ARGUMENT_VIEW_ATTRIBUTES,
        )?;
        let owner = format!("view `{}`", view.name);
        validate_policy_attribute(&owner, attribute, MODEL_ACTIONS, VIEW_DENY_ACTIONS)?;
        validate_view_block_attribute(&view.name, attribute)?;
    }
    validate_no_build_setter_collision(
        view.fields
            .iter()
            .map(|field| (field.name.as_str(), field.span)),
        "view",
        &view.name,
    )?;
    validate_no_add_setter_collision(
        view.fields.iter().map(|field| {
            (
                field.name.as_str(),
                field.span,
                matches!(field.ty.arity, TypeArity::List),
            )
        }),
        "view",
        &view.name,
    )?;

    for field in &view.fields {
        validate_field_reserved_identifier(field, "view", &view.name)?;
        // A view's rows come straight out of its SQL body — there is no
        // response-composition step that could invoke a resolver, so
        // `@computed` on a view field would be inert. Reject it loudly,
        // before the closed list gives the generic message.
        validate_computed_field_attribute(
            field,
            "view",
            &view.name,
            ComputedFieldSupport::Rejected,
        )?;
        validate_field_attributes(FieldHost::View, &view.name, field)?;
        validate_key_and_relation_attributes("view", &view.name, field)?;
    }

    // Rule 2: every source resolves to a model.
    for source in &view.sources {
        if !model_names.contains(source.name.as_str()) {
            return Err(span_error(
                format!(
                    "view `{}` references unknown source model `{}`",
                    view.name, source.name
                ),
                source.name_span,
            ));
        }
    }
    if view.sources.is_empty() {
        return Err(span_error(
            format!(
                "view `{}` must declare at least one source model via `from <Model>`",
                view.name
            ),
            view.span,
        ));
    }

    // Rule 3: at least one SQL body — and, when one is written, it has to
    // parse. `View::server_sql()`/`embedded_sql()` return `None` both for
    // "not declared" and for "declared but not a quoted string"
    // (`@@server_sql(SELECT 1)`), so distinguishing them is what turns a
    // typo into a diagnostic instead of a view that silently reads as
    // embedded-only and is skipped by the server composer. Shared finding
    // with the `query` block, whose `@@sql` uses the same extractor
    // (cratestack#867 review finding 2).
    if view.server_sql().is_none() && view.embedded_sql().is_none() {
        if view.has_sql_attribute() {
            return Err(span_error(
                format!(
                    "view `{}` has a SQL attribute whose argument is not a quoted string. Write \
                     `@@server_sql(\"SELECT …\")` for one line, or `@@server_sql(\"\"\"` … `\"\"\")` for \
                     several — and keep any other attribute on its own line, since everything up \
                     to the last `)` on the line is read as the SQL argument",
                    view.name
                ),
                view.span,
            ));
        }
        return Err(span_error(
            format!(
                "view `{}` must declare a SQL body via `@@server_sql`, `@@embedded_sql`, or `@@sql`",
                view.name
            ),
            view.span,
        ));
    }

    // Rule 4: @@materialized requires a server SQL body.
    if view.is_materialized() && view.server_sql().is_none() {
        return Err(span_error(
            format!(
                "view `{}` is `@@materialized` but has no `@@server_sql` (or `@@sql`) body — materialized views are server-only",
                view.name
            ),
            view.span,
        ));
    }

    // Rule 5: @@materialized + @@no_unique is forbidden.
    if view.is_materialized() && view.no_unique() {
        return Err(span_error(
            format!(
                "view `{}` cannot be both `@@materialized` and `@@no_unique` — concurrent refresh requires a unique index",
                view.name
            ),
            view.span,
        ));
    }

    // Rule 6: exactly one @id unless @@no_unique.
    if !view.no_unique() {
        let id_count = view
            .fields
            .iter()
            .filter(|field| field.is_primary_key())
            .count();
        if id_count == 0 {
            return Err(span_error(
                format!(
                    "view `{}` must declare exactly one `@id` field or opt out with `@@no_unique`",
                    view.name
                ),
                view.span,
            ));
        }
        if id_count > 1 {
            return Err(span_error(
                format!(
                    "view `{}` declares multiple `@id` fields; views support a single primary key",
                    view.name
                ),
                view.span,
            ));
        }
    }

    // Rule 7: @@allow action must be "read" only.
    for attr in &view.attributes {
        if !attr.raw.starts_with("@@allow") {
            continue;
        }
        let inner = attr
            .raw
            .strip_prefix("@@allow")
            .and_then(|s| s.trim().strip_prefix('('))
            .and_then(|s| s.rsplit_once(')').map(|(body, _)| body))
            .unwrap_or("");
        // Either quote, as the generator (`parse_rule_action` in
        // `cratestack-macros/src/policy/model.rs`) and every other policy
        // rule accept: `@@allow('read', …)` used to be refused here alone.
        let action = inner
            .split(',')
            .next()
            .map(|first| first.trim().trim_matches(['"', '\'']))
            .unwrap_or("");
        if action != "read" {
            return Err(span_error(
                format!(
                    "view `{}` `@@allow` only supports the `read` action (got `{action}`)",
                    view.name
                ),
                attr.span,
            ));
        }
    }

    Ok(())
}
