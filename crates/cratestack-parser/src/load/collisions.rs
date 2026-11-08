//! M13: one name declared in two files. Only where one file already refuses a
//! repeat, so splitting a valid schema never makes it invalid
//! (docs/design/multi-file-schemas.md section 9).

use std::collections::HashMap;

use cratestack_core::{ProcedureKind, Schema, SourceSpan};

use super::graph::LoadedFile;
use crate::diagnostics::SchemaError;

/// The namespaces a single file already refuses a repeat in:
/// `validate/type_names.rs` (type, enum, model, mixin, auth),
/// `validate/mod.rs` and `validate/subscriptions.rs` (procedures with
/// subscription procedures), `validate/queries.rs`, `validate/subscriptions.rs`
/// (topics) and `validate/views.rs`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Type,
    Procedure,
    Query,
    Topic,
    View,
}

type Entry<'a> = (Bucket, &'static str, &'a str, SourceSpan);

fn entries(schema: &Schema) -> Vec<Entry<'_>> {
    let mut found: Vec<Entry<'_>> = Vec::new();
    let t = Bucket::Type;
    found.extend(
        schema
            .types
            .iter()
            .map(|d| (t, "type", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .enums
            .iter()
            .map(|d| (t, "enum", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .models
            .iter()
            .map(|d| (t, "model", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .mixins
            .iter()
            .map(|d| (t, "mixin", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .auth
            .iter()
            .map(|d| (t, "auth", d.name.as_str(), d.name_span)),
    );
    found.extend(schema.procedures.iter().map(|p| {
        let kind = match p.kind {
            ProcedureKind::Mutation => "mutation procedure",
            ProcedureKind::Query => "procedure",
        };
        (Bucket::Procedure, kind, p.name.as_str(), p.name_span)
    }));
    found.extend(schema.subscription_procedures.iter().map(|p| {
        (
            Bucket::Procedure,
            "subscription procedure",
            p.name.as_str(),
            p.name_span,
        )
    }));
    found.extend(
        schema
            .queries
            .iter()
            .map(|d| (Bucket::Query, "query", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .topics
            .iter()
            .map(|d| (Bucket::Topic, "topic", d.name.as_str(), d.name_span)),
    );
    found.extend(
        schema
            .views
            .iter()
            .map(|d| (Bucket::View, "view", d.name.as_str(), d.name_span)),
    );
    found.sort_by_key(|entry| entry.3.start);
    found
}

/// The first name a later file declares that an earlier file declared in the
/// same namespace. Spans are in the schema's address space; the caller
/// attributes the error.
pub(super) fn check(files: &[LoadedFile]) -> Result<(), SchemaError> {
    let mut earlier: HashMap<(Bucket, &str), (usize, &'static str, SourceSpan)> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        let Some(parsed) = &file.parsed else { continue };
        let here = entries(&parsed.schema);
        for (bucket, kind, name, span) in &here {
            let Some(&(first, first_kind, first_span)) = earlier.get(&(*bucket, *name)) else {
                continue;
            };
            if first == index {
                continue;
            }
            let first_file = &files[first].file.relative;
            let this_file = &file.file.relative;
            return Err(SchemaError::new(
                format!(
                    "`{name}` is declared twice: `{first_kind} {name}` in `{first_file}` (line \
                     {}) and `{kind} {name}` in `{this_file}` (line {})",
                    first_span.line, span.line
                ),
                span.start..span.end,
                span.line,
            )
            .with_related(
                format!("`{first_kind} {name}` is declared here first"),
                first_span,
            ));
        }
        for (bucket, kind, name, span) in here {
            earlier.entry((bucket, name)).or_insert((index, kind, span));
        }
    }
    Ok(())
}
