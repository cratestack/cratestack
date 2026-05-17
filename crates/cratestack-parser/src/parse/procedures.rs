use std::collections::BTreeMap;

use cratestack_core::{Attribute, Procedure, ProcedureArg, ProcedureKind, SourceSpan};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, name_span_in_line, trimmed_span};
use crate::parse::procedure_docs::split_procedure_docs;
use crate::parse::types::parse_type_ref;

pub(super) fn parse_procedure(
    lines: &[Line<'_>],
    start: usize,
    docs: Vec<String>,
) -> Result<(Procedure, usize), SchemaError> {
    let line = &lines[start];
    let (kind, signature) =
        if let Some(remainder) = line.trimmed.strip_prefix("mutation procedure ") {
            (ProcedureKind::Mutation, remainder)
        } else if let Some(remainder) = line.trimmed.strip_prefix("procedure ") {
            (ProcedureKind::Query, remainder)
        } else {
            return Err(SchemaError::new(
                "expected procedure declaration",
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        };

    let open_paren = signature.find('(').ok_or_else(|| {
        SchemaError::new(
            "procedure declaration must include arguments parentheses",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let close_paren = signature.rfind(')').ok_or_else(|| {
        SchemaError::new(
            "procedure declaration must close arguments parentheses",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    let name = signature[..open_paren].trim();
    let args_src = signature[open_paren + 1..close_paren].trim();
    let return_src = signature[close_paren + 1..]
        .trim()
        .strip_prefix(':')
        .map(str::trim)
        .ok_or_else(|| {
            SchemaError::new(
                "procedure declaration must include a return type",
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })?;

    let mut attributes = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let candidate = &lines[cursor];
        if candidate.trimmed.starts_with('@') {
            attributes.push(Attribute {
                raw: candidate.trimmed.to_owned(),
                span: trimmed_span(candidate),
            });
            cursor += 1;
            continue;
        }
        if candidate.trimmed.is_empty() {
            cursor += 1;
            continue;
        }
        break;
    }

    let (procedure_docs, arg_docs) = split_procedure_docs(docs);
    let procedure_name_span = name_span_in_line(
        line,
        line.trimmed,
        if kind == ProcedureKind::Mutation {
            "mutation procedure "
        } else {
            "procedure "
        },
    )?;
    let return_type_offset = line.raw.rfind(return_src).ok_or_else(|| {
        SchemaError::new(
            "failed to locate return type in procedure declaration",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    Ok((
        Procedure {
            docs: procedure_docs,
            name: name.to_owned(),
            name_span: procedure_name_span,
            kind,
            args: parse_procedure_args(args_src, line, &arg_docs)?,
            return_type: parse_type_ref(return_src, line, return_type_offset)?,
            attributes,
            span: SourceSpan {
                start: line.start,
                end: line.start + line.raw.len(),
                line: line.number,
            },
        },
        cursor,
    ))
}

fn parse_procedure_args(
    args_src: &str,
    line: &Line<'_>,
    arg_docs: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<ProcedureArg>, SchemaError> {
    if args_src.is_empty() {
        return Ok(Vec::new());
    }

    let Some(args_offset_in_line) = line.raw.find(args_src) else {
        return Err(SchemaError::new(
            "failed to locate procedure arguments in source line",
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    };

    let mut args = Vec::new();
    let mut segment_start = 0usize;
    for segment in args_src.split(',') {
        let arg = segment.trim();
        if arg.is_empty() {
            segment_start += segment.len() + 1;
            continue;
        }

        let arg_offset_in_segment = segment.find(arg).unwrap_or_default();
        let arg_start = line.start + args_offset_in_line + segment_start + arg_offset_in_segment;
        let arg_end = arg_start + arg.len();
        let (name, ty) = arg.split_once(':').ok_or_else(|| {
            SchemaError::new(
                format!("invalid procedure argument: {arg}"),
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })?;
        let arg_name = name.trim().to_owned();
        let name_offset_in_arg = arg.find(arg_name.as_str()).unwrap_or_default();
        let name_start = arg_start + name_offset_in_arg;
        let name_end = name_start + arg_name.len();
        let type_offset_in_arg = arg.rfind(ty.trim()).ok_or_else(|| {
            SchemaError::new(
                "failed to locate procedure argument type in source line",
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })?;
        args.push(ProcedureArg {
            docs: arg_docs.get(&arg_name).cloned().unwrap_or_default(),
            name: arg_name,
            name_span: SourceSpan {
                start: name_start,
                end: name_end,
                line: line.number,
            },
            ty: parse_type_ref(ty.trim(), line, arg_start + type_offset_in_arg - line.start)?,
            span: SourceSpan {
                start: arg_start,
                end: arg_end,
                line: line.number,
            },
        });

        segment_start += segment.len() + 1;
    }

    Ok(args)
}
