//! M12: what an imported file, or a part of an imported root, may declare
//! (docs/design/multi-file-schemas.md sections 4 and 5, Q5).

use cratestack_core::{ProcedureKind, SourceSpan};

use super::graph::LoadedFile;
use crate::diagnostics::{SchemaError, span_error};

/// How a unit came to be imported: the file holding the `import` line, and
/// that line.
#[derive(Clone)]
pub(super) struct ImportEdge {
    /// Relative to the main root's directory, as shown in messages.
    pub(super) importer: String,
    pub(super) line: SourceSpan,
}

/// Refuses the first (by position) declaration that creates runtime surface.
pub(super) fn check_import_file(file: &LoadedFile, edge: &ImportEdge) -> Result<(), SchemaError> {
    let Some(parsed) = &file.parsed else {
        return Ok(());
    };
    let schema = &parsed.schema;
    let mut found: Vec<(String, SourceSpan)> = Vec::new();
    let named = |kind: &str, name: &str| format!("{kind} {name}");
    if let Some(d) = &schema.datasource {
        found.push((named("datasource", &d.name), d.span));
    }
    if let Some(a) = &schema.auth {
        found.push((named("auth", &a.name), a.span));
    }
    if let Some(span) = parsed.directives.transport_span() {
        found.push(("transport".to_owned(), span));
    }
    if let Some(m) = &schema.mcp {
        found.push(("mcp".to_owned(), m.span));
    }
    for (name, span) in &parsed.directives.extension_spans {
        found.push((named("extension", name), *span));
    }
    found.extend(
        schema
            .models
            .iter()
            .map(|d| (named("model", &d.name), d.span)),
    );
    found.extend(
        schema
            .views
            .iter()
            .map(|d| (named("view", &d.name), d.span)),
    );
    found.extend(
        schema
            .queries
            .iter()
            .map(|d| (named("query", &d.name), d.span)),
    );
    found.extend(
        schema
            .topics
            .iter()
            .map(|d| (named("topic", &d.name), d.span)),
    );
    found.extend(schema.procedures.iter().map(|p| {
        let kind = match p.kind {
            ProcedureKind::Mutation => "mutation procedure",
            ProcedureKind::Query => "procedure",
        };
        (named(kind, &p.name), p.span)
    }));
    found.extend(
        schema
            .subscription_procedures
            .iter()
            .map(|p| (named("subscription procedure", &p.name), p.span)),
    );
    let Some((what, span)) = found.into_iter().min_by_key(|(_, span)| span.start) else {
        return Ok(());
    };
    let name = &file.file.relative;
    let importer = &edge.importer;
    Err(span_error(
        format!(
            "`{what}` cannot come from an import: `{name}` is imported by `{importer}`, and an \
             imported file may declare only `type`, `enum` and `mixin`; use `part` to split one \
             schema across files"
        ),
        span,
    )
    .with_related(format!("`{name}` is imported here"), edge.line))
}
