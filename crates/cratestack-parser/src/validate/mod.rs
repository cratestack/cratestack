mod composite_attributes;
mod fields;
mod mixins_types;
mod model_attributes;
mod models;
mod pb;
mod procedures;
mod stream_attribute;
mod type_names;
mod validator_args;
mod validators;
mod views;

use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};

use self::mixins_types::{validate_auth, validate_enums, validate_mixins, validate_types};
use self::models::validate_models;
use self::procedures::{
    validate_procedure_api_version_attribute, validate_procedure_deprecated_attribute,
    validate_procedure_isolation_attribute, validate_procedure_no_rate_limit_attribute,
};
use self::stream_attribute::validate_procedure_stream_attribute;
use self::type_names::{collect_type_names, validate_type_ref};

/// The canonical scalar type names built into the `.cstack` language today.
///
/// Exposed (via [`crate::builtin_type_names`]) so downstream test suites —
/// notably the emitter/decoder round-trip coverage in `cratestack-pg` and
/// `cratestack-sqlite` (see cratestack#232) — can assert against the same
/// authoritative list the parser validates field types against, instead of
/// hand-maintaining a second copy that can silently drift the way
/// `cratestack-lsp`'s completion list once did.
pub(crate) fn builtin_type_names() -> &'static [&'static str] {
    type_names::BUILTIN_TYPES
}

pub(crate) fn validate_schema(
    path: &str,
    source: &str,
    schema: &Schema,
) -> Result<(), SchemaError> {
    let type_names = collect_type_names(schema)?;

    let mut procedure_names = BTreeSet::new();
    for procedure in &schema.procedures {
        if !procedure_names.insert(procedure.name.clone()) {
            return Err(span_error(
                format!("duplicate procedure name `{}`", procedure.name),
                procedure.span,
            ));
        }
    }

    validate_datasource(schema)?;
    validate_no_models_under_datasource_none(schema)?;

    let page_item_type_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .collect::<BTreeSet<_>>();
    // `FindMany<T>` (unlike `Page<T>`) only ever wraps a model: filtering
    // needs a real table's columns/`allowed_fields()` to validate field
    // names against, which a `type` block has none of.
    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .collect::<BTreeSet<_>>();

    validate_models(schema, &type_names, &page_item_type_names, &model_names)?;
    validate_mixins(schema, &type_names, &page_item_type_names, &model_names)?;
    validate_types(schema, &type_names, &page_item_type_names, &model_names)?;
    validate_enums(schema)?;
    validate_auth(schema, &type_names, &page_item_type_names, &model_names)?;
    validate_procedures(schema, &type_names, &page_item_type_names, &model_names)?;
    self::views::validate_views(schema)?;

    let _ = (path, source);
    Ok(())
}

fn validate_datasource(schema: &Schema) -> Result<(), SchemaError> {
    if let Some(datasource) = &schema.datasource {
        let provider = datasource_provider(schema);

        if let Some(provider) = provider
            && provider != "postgresql"
            && provider != "sqlite"
            && provider != "none"
        {
            return Err(span_error(
                format!(
                    "unsupported datasource provider `{provider}`; expected `postgresql`, `sqlite`, or `none`"
                ),
                datasource.span,
            ));
        }
    }
    Ok(())
}

/// The `provider` config entry off `schema.datasource`, with surrounding
/// quotes stripped — `None` when there's no `datasource` block at all, or
/// the block has no `provider` entry. Shared by [`validate_datasource`] and
/// [`validate_no_models_under_datasource_none`] so both read the exact same
/// value.
fn datasource_provider(schema: &Schema) -> Option<&str> {
    schema
        .datasource
        .as_ref()?
        .entries
        .iter()
        .find(|entry| entry.key == "provider")
        .map(|entry| entry.value.trim_matches('"'))
}

/// `datasource { provider = "none" }` declares a no-database, procedures-only
/// schema (cratestack#327): the whole point is that no table-backed `model`
/// exists to accidentally query against a database that was never
/// configured. Zero-model schemas are already valid today (procedures-only
/// or even zero-procedure — see `examples/rpc-procedures/schema.cstack`), so
/// this only ever rejects, never requires, a model list.
fn validate_no_models_under_datasource_none(schema: &Schema) -> Result<(), SchemaError> {
    if datasource_provider(schema) != Some("none") {
        return Ok(());
    }
    if let Some(model) = schema.models.first() {
        return Err(span_error(
            format!(
                "model `{}` is not allowed: schema declares `datasource {{ provider = \"none\" }}`, \
                 which forbids any `model` block (this schema is procedures-only, no database is \
                 configured)",
                model.name
            ),
            model.span,
        ));
    }
    Ok(())
}

fn validate_procedures(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for procedure in &schema.procedures {
        for arg in &procedure.args {
            validate_type_ref(
                type_names,
                page_item_type_names,
                model_names,
                &schema.declared_extensions,
                &arg.ty,
                procedure.span,
                self::type_names::TypeRefAllow {
                    page_input: true,
                    find_many: true,
                    ..Default::default()
                },
            )?;
        }
        validate_type_ref(
            type_names,
            page_item_type_names,
            model_names,
            &schema.declared_extensions,
            &procedure.return_type,
            procedure.span,
            self::type_names::TypeRefAllow {
                page: true,
                ..Default::default()
            },
        )?;
        validate_procedure_isolation_attribute(procedure)?;
        validate_procedure_api_version_attribute(procedure)?;
        validate_procedure_deprecated_attribute(procedure)?;
        validate_procedure_stream_attribute(procedure)?;
        validate_procedure_no_rate_limit_attribute(procedure, schema)?;
    }
    Ok(())
}
