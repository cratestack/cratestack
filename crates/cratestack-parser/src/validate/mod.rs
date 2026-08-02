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
    validate_procedure_isolation_attribute,
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

    let page_item_type_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .collect::<BTreeSet<_>>();

    validate_models(schema, &type_names, &page_item_type_names)?;
    validate_mixins(schema, &type_names, &page_item_type_names)?;
    validate_types(schema, &type_names, &page_item_type_names)?;
    validate_enums(schema)?;
    validate_auth(schema, &type_names, &page_item_type_names)?;
    validate_procedures(schema, &type_names, &page_item_type_names)?;
    self::views::validate_views(schema)?;

    let _ = (path, source);
    Ok(())
}

fn validate_datasource(schema: &Schema) -> Result<(), SchemaError> {
    if let Some(datasource) = &schema.datasource {
        let provider = datasource
            .entries
            .iter()
            .find(|entry| entry.key == "provider")
            .map(|entry| entry.value.trim_matches('"'));

        if let Some(provider) = provider
            && provider != "postgresql"
            && provider != "sqlite"
        {
            return Err(span_error(
                format!(
                    "unsupported datasource provider `{provider}`; expected `postgresql` or `sqlite`"
                ),
                datasource.span,
            ));
        }
    }
    Ok(())
}

fn validate_procedures(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for procedure in &schema.procedures {
        for arg in &procedure.args {
            validate_type_ref(
                type_names,
                page_item_type_names,
                &arg.ty,
                procedure.span,
                false,
            )?;
        }
        validate_type_ref(
            type_names,
            page_item_type_names,
            &procedure.return_type,
            procedure.span,
            true,
        )?;
        validate_procedure_isolation_attribute(procedure)?;
        validate_procedure_api_version_attribute(procedure)?;
        validate_procedure_deprecated_attribute(procedure)?;
        validate_procedure_stream_attribute(procedure)?;
    }
    Ok(())
}
