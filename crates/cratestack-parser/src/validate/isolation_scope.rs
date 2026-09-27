//! Where `@isolation("...")` may appear (docs/design/procedure-isolation.md
//! §8). The attribute makes dispatch run the procedure inside one database
//! transaction at the declared level (GHSA-r67q-4qqq-g9gm), so it is
//! refused where that cannot happen rather than accepted and ignored:
//!
//! - on a `@stream` procedure, whose items are produced after the procedure
//!   returns — there is no point at which to commit, and a stream already
//!   partly sent cannot be retried;
//! - in a `datasource { provider = "none" }` schema, which has no database.

use cratestack_core::{Procedure, Schema};

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_procedure_isolation_scope(
    procedure: &Procedure,
    schema: &Schema,
) -> Result<(), SchemaError> {
    let Some(attribute) = procedure
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@isolation"))
    else {
        return Ok(());
    };
    if procedure
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@stream")
    {
        return Err(span_error(
            format!(
                "procedure `{}` declares both @isolation and @stream: a streamed response is \
                 produced after the procedure returns, so it cannot run inside one transaction \
                 that commits (and is retried) as a unit — remove one of the two",
                procedure.name,
            ),
            attribute.span,
        ));
    }
    if super::datasource_provider(schema) == Some("none") {
        return Err(span_error(
            format!(
                "procedure `{}` declares @isolation, but this schema's datasource is \
                 `provider = \"none\"` (db = None): there is no database transaction to isolate",
                procedure.name,
            ),
            attribute.span,
        ));
    }
    Ok(())
}
