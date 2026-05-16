use cratestack_core::{Attribute, Schema, SourceSpan};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Uri};

use crate::relation_parse::parse_relation_attribute_spans;
use crate::text::{line_start_offsets, range_from_offsets};

pub(crate) fn analyze_document(uri: &Uri, text: &str) -> (Option<Schema>, Vec<Diagnostic>) {
    let label = uri
        .to_file_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.to_string());

    match cratestack_parser::parse_schema_named(&label, text) {
        Ok(schema) => (Some(schema), Vec::new()),
        Err(error) => (None, vec![schema_error_to_diagnostic(text, &error)]),
    }
}

fn schema_error_to_diagnostic(text: &str, error: &cratestack_parser::SchemaError) -> Diagnostic {
    let span = precise_relation_error_span(text, error).unwrap_or_else(|| error.span());
    Diagnostic {
        range: range_from_offsets(text, span.start, span.end),
        severity: Some(DiagnosticSeverity::ERROR),
        message: error.message().to_owned(),
        source: Some("cratestack".to_owned()),
        ..Diagnostic::default()
    }
}

fn precise_relation_error_span(
    text: &str,
    error: &cratestack_parser::SchemaError,
) -> Option<std::ops::Range<usize>> {
    let (field_name, list_key) = if let Some(name) =
        extract_message_field_name(error.message(), "unknown local field `")
    {
        (name, "fields")
    } else if let Some(name) = extract_message_field_name(error.message(), "unknown target field `")
    {
        (name, "references")
    } else {
        return None;
    };

    let line_text = text.lines().nth(error.line().saturating_sub(1))?;
    let line_start = *line_start_offsets(text).get(error.line().saturating_sub(1))?;
    let attribute = relation_attribute_from_line(line_text, line_start, error.line())?;
    let relation = parse_relation_attribute_spans(&attribute)?;
    let target = match list_key {
        "fields" => relation
            .fields
            .into_iter()
            .find(|name| name.name == field_name)?,
        "references" => relation
            .references
            .into_iter()
            .find(|name| name.name == field_name)?,
        _ => return None,
    };

    Some(target.span.start..target.span.end)
}

fn extract_message_field_name<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let suffix = message.split_once(prefix)?.1;
    suffix.split('`').next()
}

fn relation_attribute_from_line(
    line_text: &str,
    line_start: usize,
    line_number: usize,
) -> Option<Attribute> {
    let raw_start = line_text.find("@relation(")?;
    let raw = line_text[raw_start..].trim_end().to_owned();
    Some(Attribute {
        raw: raw.clone(),
        span: SourceSpan {
            start: line_start + raw_start,
            end: line_start + raw_start + raw.len(),
            line: line_number,
        },
    })
}
