use std::collections::BTreeMap;

use chumsky::prelude::*;
use cratestack_core::{
    Attribute, AuthBlock, ConfigBlock, ConfigEntry, Datasource, EnumDecl, EnumVariant, Field,
    MixinDecl, Model, Procedure, ProcedureArg, ProcedureKind, Schema, SourceSpan, TypeArity,
    TypeDecl, TypeRef,
};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{
    Line, collect_lines, name_span_in_line, parse_doc_comment, span_from_lines, split_config_entry,
    trimmed_span,
};

pub(crate) fn parse_schema_only(source: &str) -> Result<Schema, SchemaError> {
    let lines = collect_lines(source);
    let mut cursor = 0usize;
    let mut pending_docs = Vec::new();

    let mut datasource = None;
    let mut auth = None;
    let mut config_blocks = Vec::new();
    let mut mixins = Vec::new();
    let mut models = Vec::new();
    let mut types = Vec::new();
    let mut enums = Vec::new();
    let mut procedures = Vec::new();

    while cursor < lines.len() {
        let line = &lines[cursor];
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            cursor += 1;
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            cursor += 1;
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            cursor += 1;
            continue;
        }

        if line.trimmed.starts_with("datasource ") {
            let (block, next) = parse_named_config_block(&lines, cursor, "datasource")?;
            datasource = Some(Datasource {
                docs: std::mem::take(&mut pending_docs),
                name: block.name,
                entries: block
                    .entries
                    .into_iter()
                    .map(|entry| {
                        let (key, value) = split_config_entry(&entry, line)?;
                        Ok(ConfigEntry { key, value })
                    })
                    .collect::<Result<Vec<_>, SchemaError>>()?,
                span: block.span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("auth ") {
            let (name, body, span, next) = parse_body_block(&lines, cursor, "auth")?;
            auth = Some(AuthBlock {
                docs: std::mem::take(&mut pending_docs),
                name,
                fields: parse_fields(&body)?,
                span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("mixin ") {
            let (name, body, span, next) = parse_body_block(&lines, cursor, "mixin")?;
            mixins.push(MixinDecl {
                docs: std::mem::take(&mut pending_docs),
                name: name.clone(),
                name_span: name_span_in_line(line, line.trimmed, "mixin ")?,
                fields: parse_fields(&body)?,
                span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("model ") {
            let (name, body, span, next) = parse_body_block(&lines, cursor, "model")?;
            let (fields, attributes) = parse_model_body(&body)?;
            models.push(Model {
                docs: std::mem::take(&mut pending_docs),
                name,
                name_span: name_span_in_line(line, line.trimmed, "model ")?,
                fields,
                attributes,
                span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("type ") {
            let (name, body, span, next) = parse_body_block(&lines, cursor, "type")?;
            types.push(TypeDecl {
                docs: std::mem::take(&mut pending_docs),
                name,
                name_span: name_span_in_line(line, line.trimmed, "type ")?,
                fields: parse_fields(&body)?,
                span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("enum ") {
            let (name, body, span, next) = parse_body_block(&lines, cursor, "enum")?;
            enums.push(EnumDecl {
                docs: std::mem::take(&mut pending_docs),
                name,
                name_span: name_span_in_line(line, line.trimmed, "enum ")?,
                variants: parse_enum_variants(&body)?,
                span,
            });
            cursor = next;
            continue;
        }

        if line.trimmed == "mcp {" {
            let (mut block, next) = parse_simple_config_block(&lines, cursor, "mcp")?;
            block.docs = std::mem::take(&mut pending_docs);
            config_blocks.push(block);
            cursor = next;
            continue;
        }

        if line.trimmed.starts_with("procedure ") || line.trimmed.starts_with("mutation procedure ")
        {
            let (procedure, next) =
                parse_procedure(&lines, cursor, std::mem::take(&mut pending_docs))?;
            procedures.push(procedure);
            cursor = next;
            continue;
        }

        return Err(SchemaError::new(
            format!("unsupported top-level declaration: {}", line.trimmed),
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    }

    expand_model_mixins(&mixins, &mut models)?;

    Ok(Schema {
        datasource,
        auth,
        config_blocks,
        mixins,
        models,
        types,
        enums,
        procedures,
    })
}

fn parse_named_config_block(
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

fn parse_simple_config_block(
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

fn parse_body_block<'a>(
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

fn parse_model_body(lines: &[Line<'_>]) -> Result<(Vec<Field>, Vec<Attribute>), SchemaError> {
    let mut fields = Vec::new();
    let mut attributes = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("@@") || line.trimmed.starts_with("@use(") {
            pending_docs.clear();
            attributes.push(Attribute {
                raw: line.trimmed.to_owned(),
                span: trimmed_span(line),
            });
            continue;
        }
        if line.trimmed.starts_with('@') {
            return Err(SchemaError::new(
                format!("unsupported model directive `{}`", line.trimmed),
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        }
        fields.push(parse_field(line, std::mem::take(&mut pending_docs))?);
    }
    Ok((fields, attributes))
}

fn expand_model_mixins(mixins: &[MixinDecl], models: &mut [Model]) -> Result<(), SchemaError> {
    let mixins_by_name = mixins
        .iter()
        .map(|mixin| (mixin.name.clone(), mixin))
        .collect::<BTreeMap<_, _>>();

    for model in models.iter_mut() {
        let mut expanded_fields = Vec::new();
        let mut field_names = model
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut retained_attributes = Vec::new();

        for attribute in &model.attributes {
            if !attribute.raw.starts_with("@use(") {
                retained_attributes.push(attribute.clone());
                continue;
            }

            let mixin_names = parse_model_use_attribute(attribute)?;
            for mixin_name in mixin_names {
                let mixin = mixins_by_name.get(&mixin_name).ok_or_else(|| {
                    SchemaError::new(
                        format!(
                            "model `{}` references unknown mixin `{}` in {}",
                            model.name, mixin_name, attribute.raw
                        ),
                        attribute.span.start..attribute.span.end,
                        attribute.span.line,
                    )
                })?;

                for field in &mixin.fields {
                    if field_names.insert(field.name.clone()) {
                        expanded_fields.push(field.clone());
                    }
                }
            }
        }

        expanded_fields.extend(model.fields.clone());
        model.fields = expanded_fields;
        model.attributes = retained_attributes;
    }

    Ok(())
}

fn parse_model_use_attribute(attribute: &Attribute) -> Result<Vec<String>, SchemaError> {
    let Some(inner) = attribute
        .raw
        .strip_prefix("@use(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(SchemaError::new(
            format!("invalid model use attribute `{}`", attribute.raw),
            attribute.span.start..attribute.span.end,
            attribute.span.line,
        ));
    };
    let names = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(SchemaError::new(
            format!(
                "model use attribute `{}` must list at least one mixin",
                attribute.raw
            ),
            attribute.span.start..attribute.span.end,
            attribute.span.line,
        ));
    }
    Ok(names)
}

fn parse_fields(lines: &[Line<'_>]) -> Result<Vec<Field>, SchemaError> {
    let mut fields = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        fields.push(parse_field(line, std::mem::take(&mut pending_docs))?);
    }
    Ok(fields)
}

fn parse_enum_variants(lines: &[Line<'_>]) -> Result<Vec<EnumVariant>, SchemaError> {
    let mut variants = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.chars().any(char::is_whitespace) {
            return Err(SchemaError::new(
                "enum variants must be declared as a single identifier per line",
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        }
        variants.push(EnumVariant {
            docs: std::mem::take(&mut pending_docs),
            name: line.trimmed.to_owned(),
            span: trimmed_span(line),
        });
    }
    Ok(variants)
}

fn parse_field(line: &Line<'_>, docs: Vec<String>) -> Result<Field, SchemaError> {
    let mut parts = line.trimmed.splitn(3, char::is_whitespace);
    let name = parts.next().ok_or_else(|| {
        SchemaError::new(
            "expected field name",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let ty = parts.next().ok_or_else(|| {
        SchemaError::new(
            "expected field type",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let attrs = parts.next().unwrap_or_default();

    let trimmed_start = line.raw.find(line.trimmed).unwrap_or_default();
    let name_offset_in_trimmed = line.trimmed.find(name).unwrap_or_default();
    let after_name = &line.trimmed[name_offset_in_trimmed + name.len()..];
    let whitespace_after_name = after_name.len() - after_name.trim_start().len();
    let ty_offset_in_trimmed = name_offset_in_trimmed + name.len() + whitespace_after_name;
    let name_span = SourceSpan {
        start: line.start + trimmed_start + name_offset_in_trimmed,
        end: line.start + trimmed_start + name_offset_in_trimmed + name.len(),
        line: line.number,
    };
    let ty_span = SourceSpan {
        start: line.start + trimmed_start + ty_offset_in_trimmed,
        end: line.start + trimmed_start + ty_offset_in_trimmed + ty.len(),
        line: line.number,
    };
    let attrs_offset = if attrs.is_empty() {
        ty_span.end.saturating_sub(line.start)
    } else {
        line.raw
            .find(attrs)
            .unwrap_or(ty_span.end.saturating_sub(line.start))
    };
    let attribute_spans = split_field_attributes(attrs, attrs_offset);

    Ok(Field {
        docs,
        name: name.to_owned(),
        name_span,
        ty: parse_type_ref(ty, line, ty_span.start.saturating_sub(line.start))?,
        attributes: attribute_spans
            .into_iter()
            .map(|(raw, start, end)| Attribute {
                raw,
                span: SourceSpan {
                    start: line.start + start,
                    end: line.start + end,
                    line: line.number,
                },
            })
            .collect(),
        span: SourceSpan {
            start: line.start,
            end: line.start + line.raw.len(),
            line: line.number,
        },
    })
}

fn split_field_attributes(attrs: &str, offset: usize) -> Vec<(String, usize, usize)> {
    let mut attributes = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut current_start = None;

    for (index, ch) in attrs.char_indices() {
        if current.is_empty() {
            if ch == '@' {
                current.push(ch);
                current_start = Some(offset + index);
            }
            continue;
        }

        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ch if ch.is_whitespace() && depth == 0 => {
                let start = current_start.take().unwrap_or(offset + index);
                attributes.push((std::mem::take(&mut current), start, offset + index));
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        let start = current_start.unwrap_or(offset + attrs.len().saturating_sub(current.len()));
        attributes.push((current, start, offset + attrs.len()));
    }

    attributes
}

fn parse_procedure(
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

fn split_procedure_docs(docs: Vec<String>) -> (Vec<String>, BTreeMap<String, Vec<String>>) {
    let mut procedure_docs = Vec::new();
    let mut arg_docs = BTreeMap::<String, Vec<String>>::new();

    for doc in docs {
        if let Some(param) = doc.strip_prefix("@param ") {
            let mut parts = param.trim().splitn(2, char::is_whitespace);
            let Some(name) = parts.next() else {
                continue;
            };
            let description = parts.next().unwrap_or_default().trim();
            if description.is_empty() {
                continue;
            }
            arg_docs
                .entry(name.to_owned())
                .or_default()
                .push(description.to_owned());
        } else {
            procedure_docs.push(doc);
        }
    }

    (procedure_docs, arg_docs)
}

fn parse_type_ref(raw: &str, line: &Line<'_>, raw_offset: usize) -> Result<TypeRef, SchemaError> {
    let parser = text::ident::<_, extra::Err<Simple<char>>>()
        .then(choice((
            just("[]").to(TypeArity::List),
            just("?").to(TypeArity::Optional),
            end().to(TypeArity::Required),
        )))
        .then_ignore(end());

    parser
        .parse(raw)
        .into_result()
        .map(|(name, arity)| TypeRef {
            name: name.to_owned(),
            name_span: SourceSpan {
                start: line.start + raw_offset,
                end: line.start + raw_offset + name.len(),
                line: line.number,
            },
            arity,
            generic_args: Vec::new(),
        })
        .or_else(|_| parse_builtin_generic_type_ref(raw, line, raw_offset))
        .map_err(|_| {
            SchemaError::new(
                format!("invalid type reference: {raw}"),
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })
}

fn parse_builtin_generic_type_ref(
    raw: &str,
    line: &Line<'_>,
    raw_offset: usize,
) -> Result<TypeRef, ()> {
    let (base, arity) = if let Some(base) = raw.strip_suffix("[]") {
        (base.trim(), TypeArity::List)
    } else if let Some(base) = raw.strip_suffix('?') {
        (base.trim(), TypeArity::Optional)
    } else {
        (raw.trim(), TypeArity::Required)
    };

    let Some(inner) = base
        .strip_prefix("Page<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Err(());
    };

    let inner_offset = base.find('<').ok_or(())? + 1;
    let inner = parse_type_ref(inner.trim(), line, raw_offset + inner_offset).map_err(|_| ())?;
    Ok(TypeRef {
        name: "Page".to_owned(),
        name_span: SourceSpan {
            start: line.start + raw_offset,
            end: line.start + raw_offset + "Page".len(),
            line: line.number,
        },
        arity,
        generic_args: vec![inner],
    })
}
