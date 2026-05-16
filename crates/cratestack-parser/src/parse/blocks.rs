use cratestack_core::{ConfigBlock, SourceSpan, TransportStyle};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, span_from_lines};

pub(super) fn parse_transport_directive(line: &Line<'_>) -> Result<TransportStyle, SchemaError> {
    let rest = line.trimmed.strip_prefix("transport").unwrap_or("").trim();
    if rest.is_empty() {
        return Err(SchemaError::new(
            "expected transport style after `transport` (one of: rest, rpc)",
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    }
    match rest {
        "rest" => Ok(TransportStyle::Rest),
        "rpc" => Ok(TransportStyle::Rpc),
        other => Err(SchemaError::new(
            format!("unknown transport style `{other}` (expected one of: rest, rpc)"),
            line.start..line.start + line.raw.len(),
            line.number,
        )),
    }
}

pub(super) fn parse_named_config_block(
    lines: &[Line<'_>],
    start: usize,
    keyword: &str,
) -> Result<(ConfigBlock, usize), SchemaError> {
    let header = &lines[start];
    let prefix = format!("{keyword} ");
    let remainder = header.trimmed.strip_prefix(&prefix).ok_or_else(|| {
        SchemaError::new(
            format!("expected {keyword} declaration"),
            header.start..header.start + header.raw.len(),
            header.number,
        )
    })?;
    let name = remainder.strip_suffix('{').map(str::trim).ok_or_else(|| {
        SchemaError::new(
            format!("expected {keyword} block header ending with '{{'"),
            header.start..header.start + header.raw.len(),
            header.number,
        )
    })?;
    let (entries, next) = collect_block_entries(lines, start)?;

    Ok((
        ConfigBlock {
            docs: Vec::new(),
            name: name.to_owned(),
            entries,
            span: span_from_lines(header, &lines[next - 1]),
        },
        next,
    ))
}

pub(super) fn parse_simple_config_block(
    lines: &[Line<'_>],
    start: usize,
    keyword: &str,
) -> Result<(ConfigBlock, usize), SchemaError> {
    let header = &lines[start];
    if header.trimmed != format!("{keyword} {{") {
        return Err(SchemaError::new(
            format!("expected {keyword} block"),
            header.start..header.start + header.raw.len(),
            header.number,
        ));
    }

    let (entries, next) = collect_block_entries(lines, start)?;
    Ok((
        ConfigBlock {
            docs: Vec::new(),
            name: keyword.to_owned(),
            entries,
            span: span_from_lines(header, &lines[next - 1]),
        },
        next,
    ))
}

pub(super) fn parse_body_block<'a>(
    lines: &'a [Line<'a>],
    start: usize,
    keyword: &str,
) -> Result<(String, Vec<Line<'a>>, SourceSpan, usize), SchemaError> {
    let header = &lines[start];
    let prefix = format!("{keyword} ");
    let remainder = header.trimmed.strip_prefix(&prefix).ok_or_else(|| {
        SchemaError::new(
            format!("expected {keyword} declaration"),
            header.start..header.start + header.raw.len(),
            header.number,
        )
    })?;
    let name = remainder.strip_suffix('{').map(str::trim).ok_or_else(|| {
        SchemaError::new(
            format!("expected {keyword} block header ending with '{{'"),
            header.start..header.start + header.raw.len(),
            header.number,
        )
    })?;

    let mut body = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = &lines[cursor];
        if line.trimmed == "}" {
            return Ok((
                name.to_owned(),
                body,
                span_from_lines(header, line),
                cursor + 1,
            ));
        }
        body.push(line.clone());
        cursor += 1;
    }

    Err(SchemaError::new(
        format!("unterminated {keyword} block"),
        header.start..header.start + header.raw.len(),
        header.number,
    ))
}

fn collect_block_entries(
    lines: &[Line<'_>],
    start: usize,
) -> Result<(Vec<String>, usize), SchemaError> {
    let mut entries = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = &lines[cursor];
        if line.trimmed == "}" {
            return Ok((entries, cursor + 1));
        }
        if !line.trimmed.is_empty() && !line.trimmed.starts_with("//") {
            entries.push(line.trimmed.to_owned());
        }
        cursor += 1;
    }

    let header = &lines[start];
    Err(SchemaError::new(
        "unterminated config block",
        header.start..header.start + header.raw.len(),
        header.number,
    ))
}
