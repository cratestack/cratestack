//! `onDelete`/`onUpdate` referential-action vocabulary and validation,
//! split out of `relation_helpers` to stay under the 200-LoC budget.

use cratestack_core::{Field, Model, TypeArity};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_helpers::ParsedRelationAttribute;

/// `ON DELETE`/`ON UPDATE` referential action from `@relation(...,
/// onDelete: ..., onUpdate: ...)`. Vocabulary matches the SQL
/// standard keywords (Prisma uses the same names for the same
/// concepts), spelled as bareword identifiers to match this schema
/// language's existing convention (`fields:[authorId]`, not
/// `fields:["authorId"]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationAction {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    NoAction,
}

pub(crate) fn parse_relation_action(value: &str) -> Result<RelationAction, String> {
    match value {
        "Cascade" => Ok(RelationAction::Cascade),
        "Restrict" => Ok(RelationAction::Restrict),
        "SetNull" => Ok(RelationAction::SetNull),
        "SetDefault" => Ok(RelationAction::SetDefault),
        "NoAction" => Ok(RelationAction::NoAction),
        other => Err(format!(
            "invalid relation action `{other}` — expected one of Cascade, Restrict, SetNull, SetDefault, NoAction"
        )),
    }
}

/// Validates `onDelete`/`onUpdate`, once the relation itself is known
/// to resolve. Two rules, both mirroring what Postgres itself would
/// reject at `ADD CONSTRAINT` time — surfacing them at `check` time
/// instead means a schema author sees the real problem, not a runtime
/// migration failure with no context back to the `.cstack` source:
///
/// * The action can only be declared on the relation's owning side —
///   the `List`-typed "many" side has no physical column of its own,
///   so there's no constraint for the action to attach to.
/// * `SetNull` requires the local FK column to be optional; `SetDefault`
///   requires it to declare `@default(...)`. Postgres enforces both at
///   `ADD CONSTRAINT` time regardless of which action (`onDelete` or
///   `onUpdate`) triggers it.
pub(crate) fn validate_relation_actions(
    relation_field: &Field,
    model: &Model,
    local_field: &Field,
    relation: &ParsedRelationAttribute,
) -> Result<(), SchemaError> {
    if relation_field.ty.arity == TypeArity::List {
        if relation.on_delete.is_some() || relation.on_update.is_some() {
            return Err(span_error(
                format!(
                    "relation field `{}` on model `{}` cannot declare onDelete/onUpdate — \
                     it is the has-many side of the relation and owns no column; declare the \
                     action on the owning side instead",
                    relation_field.name, model.name,
                ),
                relation_field.span,
            ));
        }
        return Ok(());
    }

    for (label, action) in [
        ("onDelete", relation.on_delete),
        ("onUpdate", relation.on_update),
    ] {
        match action {
            Some(RelationAction::SetNull) if local_field.ty.arity != TypeArity::Optional => {
                return Err(span_error(
                    format!(
                        "relation field `{}` on model `{}` declares {label}: SetNull, but local field `{}` is not optional",
                        relation_field.name, model.name, local_field.name,
                    ),
                    relation_field.span,
                ));
            }
            Some(RelationAction::SetDefault) if !field_has_default(local_field) => {
                return Err(span_error(
                    format!(
                        "relation field `{}` on model `{}` declares {label}: SetDefault, but local field `{}` has no @default(...)",
                        relation_field.name, model.name, local_field.name,
                    ),
                    relation_field.span,
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn field_has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default("))
}
