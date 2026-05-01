use cratestack_core::SourceSpan;

use crate::diagnostics::SchemaError;

#[derive(Debug, Clone)]
pub(crate) struct Line<'a> {
    pub(crate) raw: &'a str,
    pub(crate) trimmed: &'a str,
    pub(crate) number: usize,
    pub(crate) start: usize,
}

pub(crate) fn collect_lines(source: &str) -> Vec<Line<'_>> {
    let mut offset = 0usize;
    let mut lines = Vec::new();

    for (index, raw) in source.lines().enumerate() {
        lines.push(Line {
            raw,
            trimmed: raw.trim(),
            number: index + 1,
            start: offset,
        });
        offset += raw.len() + 1;
    }

    lines
}

pub(crate) fn parse_doc_comment<'a>(line: &'a Line<'a>) -> Option<&'a str> {
    line.trimmed.strip_prefix("///").map(|doc| doc.trim_start())
}

pub(crate) fn trimmed_span(line: &Line<'_>) -> SourceSpan {
    let trimmed_start = line.raw.find(line.trimmed).unwrap_or_default();
    SourceSpan {
        start: line.start + trimmed_start,
        end: line.start + trimmed_start + line.trimmed.len(),
        line: line.number,
    }
}

pub(crate) fn token_span_in_line(line: &Line<'_>, token: &str) -> Result<SourceSpan, SchemaError> {
    let Some(relative_start) = line.raw.find(token) else {
        return Err(SchemaError::new(
            format!("failed to locate token `{token}` in source line"),
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    };
    Ok(SourceSpan {
        start: line.start + relative_start,
        end: line.start + relative_start + token.len(),
        line: line.number,
    })
}

pub(crate) fn name_span_in_line(
    line: &Line<'_>,
    trimmed: &str,
    prefix: &str,
) -> Result<SourceSpan, SchemaError> {
    let remainder = trimmed.strip_prefix(prefix).ok_or_else(|| {
        SchemaError::new(
            format!("expected declaration prefix `{prefix}`"),
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let name = remainder
        .strip_suffix('{')
        .map(str::trim)
        .unwrap_or_else(|| remainder.split('(').next().unwrap_or_default().trim());
    token_span_in_line(line, name)
}

pub(crate) fn split_config_entry(
    entry: &str,
    line: &Line<'_>,
) -> Result<(String, String), SchemaError> {
    let (key, value) = entry.split_once('=').ok_or_else(|| {
        SchemaError::new(
            format!("invalid config entry: {entry}"),
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}

pub(crate) fn span_from_lines(start: &Line<'_>, end: &Line<'_>) -> SourceSpan {
    SourceSpan {
        start: start.start,
        end: end.start + end.raw.len(),
        line: start.number,
    }
}
