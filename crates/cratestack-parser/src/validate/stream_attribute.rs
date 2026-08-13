//! Validate the bare `@stream` procedure attribute — see
//! `docs/design/rpc-transport.md` §2.1/§3.3 and cratestack#282.
//!
//! `@stream` opts a list-returning (`T[]`) `@procedure` into a
//! stream-shaped `ProcedureRegistry` trait method (see
//! `cratestack-macros/src/procedure.rs`) instead of the default buffered
//! `Future<Output = Result<Vec<T>, _>>`. It carries no arguments — unlike
//! `@isolation("...")` / `@api_version("...")`, there is nothing to parse
//! beyond presence, so this module's only job is the arity gate: `@stream`
//! is meaningless (and rejected) on a procedure that doesn't return a list,
//! since there is nothing to stream one item at a time.

use cratestack_core::TypeArity;

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_procedure_stream_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw == "@stream")
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @stream attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    if procedure.return_type.arity != TypeArity::List {
        return Err(span_error(
            format!(
                "procedure `{}` declares @stream but does not return a list type (`T[]`); \
                 @stream is only valid on procedures that return a list, since it changes how \
                 items in that list are delivered, not whether a list is returned",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}
