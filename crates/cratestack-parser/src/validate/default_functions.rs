use cratestack_core::schema::default_functions::{
    BuiltinDefault, DefaultValue, FunctionCall, classify_default, registered_functions,
};
use cratestack_core::schema::default_markers::{DefaultMarker, default_inner};
use cratestack_core::{Datasource, Field, Schema, TypeArity};

use super::default_markers::marker_error;
use crate::diagnostics::{SchemaError, span_error};

/// Check what a field's `@default(...)` calls (cratestack#1209).
///
/// A function-shaped default reaches the migration's `DEFAULT` clause, so
/// it must name something the database runs: a marker (see
/// `marker_error`), one of cratestack's own `now()`, `uuid()`, `cuid()`
/// on a field of the type it generates, or a function the datasource
/// registers with `functions = [...]`. Everything else used to pass
/// `check` and fail when the migration was applied.
pub(super) fn validate_default(
    schema: &Schema,
    owner: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let Some(inner) = default_inner(field) else {
        return Ok(());
    };
    let message = match classify_default(inner) {
        DefaultValue::Marker(call) => marker_error(owner, field, inner, call),
        DefaultValue::Auth => None,
        DefaultValue::Builtin(builtin, call) => builtin_error(owner, field, builtin, call),
        DefaultValue::Function(call) => function_error(schema, owner, field, inner, call),
        DefaultValue::Literal => bare_builtin_error(owner, field, inner),
    };
    match message {
        Some(message) => Err(span_error(message, field.span)),
        None => Ok(()),
    }
}

fn builtin_error(
    owner: &str,
    field: &Field,
    builtin: BuiltinDefault,
    call: FunctionCall<'_>,
) -> Option<String> {
    let name = builtin.name();
    let at = format!("field `{owner}.{}`", field.name);
    if call.name != name {
        return Some(format!(
            "{at} uses `@default({}(...))`; cratestack's default function is spelled \
             exactly `{name}()`.",
            call.name
        ));
    }
    if !call.is_bare() {
        return Some(format!(
            "{at} uses `@default({name}({}){})`; `{name}()` takes no argument and must be \
             the whole default expression.",
            call.args, call.trailing
        ));
    }
    let types = builtin.field_types();
    let list = matches!(field.ty.arity, TypeArity::List);
    if list || !types.contains(&field.ty.name.as_str()) {
        return Some(format!(
            "{at} is `{}{}`, but `@default({name}())` generates a single `{}` value.",
            field.ty.name,
            if list { "[]" } else { "" },
            types.join("` or `"),
        ));
    }
    None
}

fn function_error(
    schema: &Schema,
    owner: &str,
    field: &Field,
    inner: &str,
    call: FunctionCall<'_>,
) -> Option<String> {
    let at = format!("field `{owner}.{}` uses `@default({inner})`", field.name);
    if call.args.contains("auth(") || call.trailing.contains("auth(") {
        return Some(format!(
            "{at}; `auth()` is not a database function, so it cannot be part of a database \
             expression. An auth default is exactly `@default(auth().field)`."
        ));
    }
    let registered = registered_functions(schema.datasource.as_ref()).unwrap_or_default();
    if registered.contains(&call.name) {
        return None;
    }
    let hint = match call.name.to_ascii_lowercase().as_str() {
        "gen_random_uuid" | "uuid_generate_v4" => {
            " For a random UUID, `uuid()` is built in: it becomes `gen_random_uuid()` on \
             Postgres and also works on SQLite and the embedded runtime."
        }
        "current_timestamp"
        | "clock_timestamp"
        | "statement_timestamp"
        | "transaction_timestamp" => " For the current time, `now()` is built in.",
        _ => "",
    };
    Some(format!(
        "{at}, but `{}` is neither one of cratestack's default functions (`now()`, `uuid()`, \
         `cuid()`) nor registered with the datasource.{hint} If your database provides \
         `{}`, register it so `check` knows it exists: `datasource db {{ functions = \
         [\"{}\"] }}`. It must then exist before the migration that uses it runs.",
        call.name, call.name, call.name,
    ))
}

/// `@default(now)` on a `DateTime`: written to the migration as a literal.
fn bare_builtin_error(owner: &str, field: &Field, inner: &str) -> Option<String> {
    let builtin = BuiltinDefault::from_name_ignore_case(inner.trim())?;
    if !builtin.field_types().contains(&field.ty.name.as_str()) {
        return None;
    }
    Some(format!(
        "field `{owner}.{}` uses `@default({inner})`; write `@default({}())`. Without the \
         parentheses it is a literal, which the migration would write as `DEFAULT {inner}`.",
        field.name,
        builtin.name(),
    ))
}

/// Check `datasource { functions = [...] }`: a list of quoted function
/// names, none of them one cratestack defines itself.
pub(super) fn validate_registered_functions(datasource: &Datasource) -> Result<(), SchemaError> {
    let names = registered_functions(Some(datasource))
        .map_err(|message| span_error(format!("datasource: {message}"), datasource.span))?;
    for name in names {
        let reserved = BuiltinDefault::from_name_ignore_case(name)
            .map(BuiltinDefault::name)
            .or_else(|| {
                [DefaultMarker::DbGenerated, DefaultMarker::Autoincrement]
                    .into_iter()
                    .map(DefaultMarker::name)
                    .chain(["auth"])
                    .find(|reserved| name.eq_ignore_ascii_case(reserved))
            });
        if let Some(reserved) = reserved {
            return Err(span_error(
                format!(
                    "datasource registers the function `{name}`, but `{reserved}()` is \
                     cratestack's own and cannot be registered; remove it from `functions`."
                ),
                datasource.span,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "default_functions_tests.rs"]
mod tests;
