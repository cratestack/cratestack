use cratestack_core::{Attribute, SourceSpan};

use crate::state::{ParsedRelationAttributeSpans, SpannedName};

pub(crate) fn relation_attribute_spans(
    attributes: &[Attribute],
) -> Option<ParsedRelationAttributeSpans> {
    let attribute = attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("))?;
    parse_relation_attribute_spans(attribute)
}

pub(crate) fn parse_relation_attribute_spans(
    attribute: &Attribute,
) -> Option<ParsedRelationAttributeSpans> {
    let raw = attribute.raw.trim();
    let inner = raw.strip_prefix("@relation(")?.strip_suffix(')')?;
    let inner_offset = attribute.raw.find('(')? + 1;
    let mut fields = None;
    let mut references = None;

    for (entry, start, _) in split_top_level_ranges(inner, ',', inner_offset) {
        let colon = entry.find(':')?;
        let key = entry[..colon].trim();
        let value = entry[colon + 1..].trim();
        let value_offset = start + colon + 1 + entry[colon + 1..].find(value).unwrap_or_default();
        match key {
            "fields" => {
                fields = Some(parse_relation_name_list(
                    value,
                    attribute.span.line,
                    attribute.span.start + value_offset,
                )?)
            }
            "references" => {
                references = Some(parse_relation_name_list(
                    value,
                    attribute.span.line,
                    attribute.span.start + value_offset,
                )?)
            }
            _ => {}
        }
    }

    Some(ParsedRelationAttributeSpans {
        fields: fields?,
        references: references?,
    })
}

fn parse_relation_name_list(
    value: &str,
    line: usize,
    absolute_start: usize,
) -> Option<Vec<SpannedName>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let list_start = absolute_start + 1;
    let mut names = Vec::new();
    for (entry, start, end) in split_top_level_ranges(inner, ',', 0) {
        if entry.is_empty() {
            continue;
        }
        names.push(SpannedName {
            name: entry.to_owned(),
            span: SourceSpan {
                start: list_start + start,
                end: list_start + end,
                line,
            },
        });
    }
    Some(names)
}

fn split_top_level_ranges(
    input: &str,
    separator: char,
    offset: usize,
) -> Vec<(String, usize, usize)> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                let raw = &input[start..index];
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    let trim_start = raw.find(trimmed).unwrap_or_default();
                    entries.push((
                        trimmed.to_owned(),
                        offset + start + trim_start,
                        offset + start + trim_start + trimmed.len(),
                    ));
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let raw = &input[start..];
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        let trim_start = raw.find(trimmed).unwrap_or_default();
        entries.push((
            trimmed.to_owned(),
            offset + start + trim_start,
            offset + start + trim_start + trimmed.len(),
        ));
    }
    entries
}
