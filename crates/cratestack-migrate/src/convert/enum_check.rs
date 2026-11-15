//! The CHECK constraint that stands in for an enum column's native type.

use std::collections::BTreeMap;

use cratestack_core::EnumDecl;

use crate::ir::{CheckKind, Column, ColumnArity, ColumnType};

/// The membership constraint that stands in for a native enum type on
/// an enum-typed column (issue #228). Returns `None` for non-enum
/// columns, and for the degenerate case of an enum declared with no
/// variants — `IN ()` is not valid SQL, and there is nothing useful to
/// constrain.
pub(super) fn enum_check_kind(
    column: &Column,
    enum_variants: &BTreeMap<&str, &EnumDecl>,
) -> Option<CheckKind> {
    let (ColumnType::Enum(name) | ColumnType::OrdinalEnum(name)) = &column.ty else {
        return None;
    };
    let decl = enum_variants.get(name.as_str())?;
    if decl.variants.is_empty() {
        return None;
    }
    // An ordinal enum's CHECK lists its numbers, which is what the column
    // holds (`docs/design/enum-ordinals.md` §2.2).
    let ordinals = decl
        .is_ordinal()
        .then(|| decl.variants.iter().filter_map(|v| v.ordinal).collect());
    Some(CheckKind::Enum {
        variants: decl.variants.iter().map(|v| v.name.clone()).collect(),
        list: matches!(column.arity, ColumnArity::List),
        ordinals,
    })
}
