mod blocks;
mod fields;
mod models;
mod procedure_docs;
mod procedures;
mod types;

use cratestack_core::{
    AuthBlock, ConfigEntry, Datasource, EnumDecl, MixinDecl, Model, Schema, TransportStyle,
    TypeDecl,
};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{collect_lines, name_span_in_line, parse_doc_comment, split_config_entry};

use self::blocks::{
    parse_body_block, parse_named_config_block, parse_simple_config_block,
    parse_transport_directive,
};
use self::fields::{parse_enum_variants, parse_fields};
use self::models::{expand_model_mixins, parse_model_body};
use self::procedures::parse_procedure;

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
    let mut transport: Option<TransportStyle> = None;
    let mut transport_line: Option<usize> = None;

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

        if line.trimmed.starts_with("transport ") || line.trimmed == "transport" {
            let style = parse_transport_directive(line)?;
            if let Some(prev) = transport_line {
                return Err(SchemaError::new(
                    format!("duplicate `transport` directive (first declared on line {prev})"),
                    line.start..line.start + line.raw.len(),
                    line.number,
                ));
            }
            transport = Some(style);
            transport_line = Some(line.number);
            pending_docs.clear();
            cursor += 1;
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
        transport: transport.unwrap_or_default(),
    })
}
