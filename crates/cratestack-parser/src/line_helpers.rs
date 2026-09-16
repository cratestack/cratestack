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

    for (index, segment) in source.split_inclusive('\n').enumerate() {
        let raw = segment
            .strip_suffix('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .unwrap_or(segment);
        lines.push(Line {
            raw,
            trimmed: raw.trim(),
            number: index + 1,
            start: offset,
        });
        offset += segment.len();
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
    // Locate the name after its declaration prefix, not an earlier occurrence
    // inside that prefix (for example `datasource source` or `auth auth`).
    let indentation = line.raw.len() - line.raw.trim_start().len();
    let name_padding = remainder.len() - remainder.trim_start().len();
    let start = line.start + indentation + prefix.len() + name_padding;
    Ok(SourceSpan {
        start,
        end: start + name.len(),
        line: line.number,
    })
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

#[cfg(test)]
mod tests {
    use super::collect_lines;

    #[test]
    fn line_collection_preserves_str_lines_semantics_and_source_offsets() {
        for source in ["", "\n", "\r\n", "é\r\n\nlast\r", "é\nlast", "a\r\nb\n"] {
            let lines = collect_lines(source);
            assert_eq!(
                lines.iter().map(|line| line.raw).collect::<Vec<_>>(),
                source.lines().collect::<Vec<_>>()
            );
            for (index, line) in lines.iter().enumerate() {
                assert_eq!(&source[line.start..line.start + line.raw.len()], line.raw);
                assert_eq!(line.number, index + 1);
            }
        }
    }
}
