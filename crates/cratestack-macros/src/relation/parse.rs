//! `@relation(fields:[...], references:[...])` attribute parser +
//! the bracket-aware `,` splitter it uses on the body.

use cratestack_core::Field;

use super::types::ParsedRelationAttribute;

pub(crate) fn parse_relation_attribute(field: &Field) -> Option<ParsedRelationAttribute> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("))?
        .raw
        .as_str();
    let inner = raw.strip_prefix("@relation(")?.strip_suffix(')')?;

    let mut fields = None;
    let mut references = None;
    for entry in split_top_level(inner, ',') {
        let (key, value) = entry.split_once(':')?;
        match key.trim() {
            "fields" => fields = Some(parse_relation_list(value.trim())?),
            "references" => references = Some(parse_relation_list(value.trim())?),
            _ => return None,
        }
    }

    Some(ParsedRelationAttribute {
        fields: fields?,
        references: references?,
    })
}

fn parse_relation_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let values = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

pub(super) fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                entries.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    entries.push(input[start..].trim());
    entries
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .collect()
}
