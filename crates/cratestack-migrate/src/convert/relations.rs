//! Promote a `@relation(fields:[...], references:[...])` field into a
//! foreign-key IR entry.
//!
//! Only the "owning" side of a relation carries a physical column: a
//! `Model[]` back-reference (the "many" side) has no local column of
//! its own and produces no constraint. `cratestack-parser` requires
//! *both* sides of a relation to declare `@relation(...)`, using the
//! same `fields`/`references` shape either way — `fields` names local
//! columns, `references` names columns on the target model — so the
//! only thing that distinguishes the owning side is arity: a `List`
//! field is the inverse accessor, not a real foreign key.

use cratestack_core::{Field, Schema, TypeArity};

use crate::ir::AddForeignKey;
use crate::naming::{column_name, fk_name, table_name};

pub(super) fn relation_foreign_key(
    field: &Field,
    schema: &Schema,
    table: &str,
) -> Option<AddForeignKey> {
    if field.ty.arity == TypeArity::List {
        return None;
    }
    let attribute = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("))?;
    let relation = parse_relation_attribute(&attribute.raw)?;
    let local_field = relation.fields.first()?;
    let target_field = relation.references.first()?;
    let target_model = schema.models.iter().find(|m| m.name == field.ty.name)?;

    let column = column_name(local_field);
    Some(AddForeignKey {
        name: fk_name(table, &column),
        table: table.to_owned(),
        column,
        referenced_table: table_name(&target_model.name),
        referenced_column: column_name(target_field),
    })
}

struct ParsedRelationAttribute {
    fields: Vec<String>,
    references: Vec<String>,
}

/// Parses `@relation(fields:[...],references:[...])`. Mirrors
/// `cratestack-parser::relation_helpers` and
/// `cratestack-macros::relation::parse`, which are crate-private to
/// their own crates — `cratestack-migrate` doesn't depend on either,
/// so it re-derives the same small parse here rather than taking on a
/// dependency solely to reuse ~20 lines of string splitting.
fn parse_relation_attribute(raw: &str) -> Option<ParsedRelationAttribute> {
    let inner = raw.strip_prefix("@relation(")?.strip_suffix(')')?;
    let mut fields = None;
    let mut references = None;
    for entry in split_top_level(inner) {
        let (key, value) = entry.split_once(':')?;
        match key.trim() {
            "fields" => fields = Some(parse_bracket_list(value.trim())?),
            "references" => references = Some(parse_bracket_list(value.trim())?),
            // Ignore any other key rather than dropping the whole
            // relation. `cratestack-parser` is the sole vocabulary
            // gatekeeper — it rejects a genuinely unknown key with a
            // diagnostic before a schema ever reaches this crate — so
            // by the time this runs, an unrecognised key here just
            // means the parser's vocabulary has grown past what this
            // FK-only parser cares about (e.g. `onDelete`/`onUpdate`
            // before support for them existed here). Returning `None`
            // silently dropped the foreign key entirely with no error
            // at all — worse than ignoring the extra key.
            _ => {}
        }
    }
    Some(ParsedRelationAttribute {
        fields: fields?,
        references: references?,
    })
}

fn parse_bracket_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let values: Vec<String> = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (!values.is_empty()).then_some(values)
}

fn split_top_level(input: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                entries.push(input[start..index].trim());
                start = index + 1;
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
