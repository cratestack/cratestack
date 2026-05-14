//! Compute a list of [`Op`]s that turns one [`Schema`] into another.
//!
//! The algorithm is deliberately conservative:
//!
//! * Tables and columns are matched **by name only**. Renames are not
//!   inferred from text — they must be declared via `@rename` (slice
//!   9). A column that disappears and a new column that appears look
//!   exactly the same here, and the engine treats them as drop + add.
//! * Column *changes* (type, nullability, default) are not yet
//!   detected — slice 8 adds `AlterColumn*` ops.
//! * Index changes follow the same drop/add pattern.
//!
//! Ops are emitted in an order that respects DDL dependencies:
//! drops first (with dependent index drops before column/table drops),
//! then creates, then index adds (after the columns that back them
//! exist).

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Schema;

use crate::convert::{TableProjection, project_model};
use crate::ir::{
    AddColumn, AlterColumnDefault, AlterColumnNullability, AlterColumnType, AlterEnumAddVariant,
    Column, CreateEnum, DropCheck, DropColumn, DropEnum, DropIndex, DropTable, Op, RenameColumn,
    RenameTable,
};

/// Compute the migration that turns `prev` into `next`.
///
/// Returns an empty vector when the schemas are equivalent at the
/// resolution this slice supports (no column-shape changes detected,
/// only structural ones).
pub fn diff(prev: &Schema, next: &Schema) -> Vec<Op> {
    let prev_tables: BTreeMap<String, TableProjection> = prev
        .models
        .iter()
        .map(|model| {
            let projection = project_model(model, prev);
            (projection.name.clone(), projection)
        })
        .collect();
    let next_tables: BTreeMap<String, TableProjection> = next
        .models
        .iter()
        .map(|model| {
            let projection = project_model(model, next);
            (projection.name.clone(), projection)
        })
        .collect();

    let mut create_enums: Vec<Op> = Vec::new();
    let mut alter_enums: Vec<Op> = Vec::new();
    let mut drop_enums: Vec<Op> = Vec::new();
    let mut rename_tables: Vec<Op> = Vec::new();
    let mut rename_columns: Vec<Op> = Vec::new();
    let mut drop_indexes: Vec<Op> = Vec::new();
    let mut drop_columns: Vec<Op> = Vec::new();
    let mut drop_tables: Vec<Op> = Vec::new();
    let mut create_tables: Vec<Op> = Vec::new();
    let mut add_columns: Vec<Op> = Vec::new();
    let mut alter_columns: Vec<Op> = Vec::new();
    let mut add_indexes: Vec<Op> = Vec::new();
    let mut add_checks: Vec<Op> = Vec::new();
    let mut drop_checks: Vec<Op> = Vec::new();

    // Enums first — tables that reference an enum need the type to
    // exist before CREATE TABLE runs.
    {
        let prev_enums: BTreeMap<&str, Vec<&str>> = prev
            .enums
            .iter()
            .map(|decl| {
                (
                    decl.name.as_str(),
                    decl.variants.iter().map(|v| v.name.as_str()).collect(),
                )
            })
            .collect();
        let next_enums: BTreeMap<&str, Vec<&str>> = next
            .enums
            .iter()
            .map(|decl| {
                (
                    decl.name.as_str(),
                    decl.variants.iter().map(|v| v.name.as_str()).collect(),
                )
            })
            .collect();

        for (name, _) in &prev_enums {
            if !next_enums.contains_key(name) {
                drop_enums.push(Op::DropEnum(DropEnum {
                    name: (*name).to_owned(),
                }));
            }
        }
        for (name, variants) in &next_enums {
            match prev_enums.get(name) {
                None => create_enums.push(Op::CreateEnum(CreateEnum {
                    name: (*name).to_owned(),
                    variants: variants.iter().map(|v| (*v).to_owned()).collect(),
                })),
                Some(prev_variants) => {
                    // Variants present in next but not prev → ADD VALUE.
                    // Variant removal is out of scope for this slice
                    // (requires the Postgres swap-dance, plus a
                    // backfill plan for referencing rows).
                    let prev_set: std::collections::HashSet<&str> =
                        prev_variants.iter().copied().collect();
                    for variant in variants {
                        if !prev_set.contains(variant) {
                            alter_enums.push(Op::AlterEnumAddVariant(AlterEnumAddVariant {
                                name: (*name).to_owned(),
                                value: (*variant).to_owned(),
                            }));
                        }
                    }
                }
            }
        }
    }

    // Resolve table renames first. A model in `next` whose
    // `@@rename(from = "...")` points at a table name that exists in
    // `prev` and whose own new name does not exist in `prev` matches
    // an existing table; we emit RenameTable and pair the projections
    // for the column-level diff below.
    let mut renamed_from: BTreeMap<&str, &str> = BTreeMap::new();
    for (new_name, projection) in &next_tables {
        let Some(old_name) = projection.rename_from.as_deref() else {
            continue;
        };
        if !prev_tables.contains_key(old_name) {
            continue;
        }
        if prev_tables.contains_key(new_name.as_str()) {
            // The new name already exists in prev — this is not a
            // rename, it's a collision. Fall through to drop+add.
            continue;
        }
        rename_tables.push(Op::RenameTable(RenameTable {
            from: old_name.to_owned(),
            to: new_name.clone(),
        }));
        renamed_from.insert(new_name.as_str(), old_name);
    }

    // Tables removed entirely (excluding ones consumed by a rename).
    let consumed_old: BTreeSet<&str> = renamed_from.values().copied().collect();
    for (name, _projection) in &prev_tables {
        if consumed_old.contains(name.as_str()) {
            continue;
        }
        if !next_tables.contains_key(name) {
            drop_tables.push(Op::DropTable(DropTable { name: name.clone() }));
        }
    }

    // Tables added entirely (excluding ones produced by a rename).
    for (name, projection) in &next_tables {
        if renamed_from.contains_key(name.as_str()) {
            continue;
        }
        if !prev_tables.contains_key(name) {
            create_tables.push(Op::CreateTable(crate::ir::CreateTable {
                name: name.clone(),
                columns: projection.columns.clone(),
            }));
            for index in &projection.indexes {
                add_indexes.push(Op::AddIndex(index.clone()));
            }
            for check in &projection.checks {
                add_checks.push(Op::AddCheck(check.clone()));
            }
        }
    }

    // Tables present in both (directly or via rename) — column- and
    // index-level diff.
    for (name, prev_projection) in &prev_tables {
        // Find the next projection by direct name or by rename map.
        let next_projection = match next_tables.get(name) {
            Some(projection) => projection,
            None => {
                // Maybe a rename: look up by inverse map.
                let renamed_new = renamed_from
                    .iter()
                    .find_map(|(new, old)| (*old == name.as_str()).then_some(*new));
                match renamed_new.and_then(|new| next_tables.get(new)) {
                    Some(projection) => projection,
                    None => continue,
                }
            }
        };

        let prev_columns: BTreeMap<_, _> = prev_projection
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column))
            .collect();
        let next_columns: BTreeMap<_, _> = next_projection
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column))
            .collect();

        // The table name to put on emitted ops is the *new* name —
        // RenameTable lands earlier in the migration, so all
        // subsequent column ops should reference the post-rename
        // table name.
        let effective_table = next_projection.name.as_str();

        // Column-level renames: a next-side column with
        // `@rename(from = "...")` consumes the matching prev-side
        // column so we don't emit drop+add for it.
        let column_renamed_from: BTreeMap<&str, &str> = next_projection
            .column_renames
            .iter()
            .filter_map(|(new, old)| {
                if prev_columns.contains_key(old.as_str())
                    && !prev_columns.contains_key(new.as_str())
                    && next_columns.contains_key(new.as_str())
                {
                    Some((new.as_str(), old.as_str()))
                } else {
                    None
                }
            })
            .collect();
        let consumed_prev_columns: BTreeSet<&str> =
            column_renamed_from.values().copied().collect();

        for (new_name, old_name) in &column_renamed_from {
            rename_columns.push(Op::RenameColumn(RenameColumn {
                table: effective_table.to_owned(),
                from: (*old_name).to_owned(),
                to: (*new_name).to_owned(),
            }));
        }

        for (column_name, _) in &prev_columns {
            if consumed_prev_columns.contains(column_name) {
                continue;
            }
            if !next_columns.contains_key(column_name) {
                drop_columns.push(Op::DropColumn(DropColumn {
                    table: effective_table.to_owned(),
                    column: (*column_name).to_owned(),
                }));
            }
        }

        for (column_name, column) in &next_columns {
            if column_renamed_from.contains_key(column_name) {
                continue;
            }
            if !prev_columns.contains_key(column_name) {
                add_columns.push(Op::AddColumn(AddColumn {
                    table: effective_table.to_owned(),
                    column: (*column).clone(),
                }));
            }
        }

        // Columns present in both — emit alter ops for shape changes.
        // Includes columns that were renamed: their identity (the
        // value the user means) is preserved, so a type or default
        // change on a renamed column still produces an alter op.
        for (column_name, prev_column) in &prev_columns {
            // If this prev-column was consumed by a rename, compare
            // against the *new* column on the next side.
            let renamed_to = column_renamed_from
                .iter()
                .find_map(|(new, old)| (*old == *column_name).then_some(*new));
            let next_column = match renamed_to {
                Some(new_name) => next_columns.get(new_name),
                None => next_columns.get(column_name),
            };
            let Some(next_column) = next_column else {
                continue;
            };
            // For alter ops, use the *new* column name so they line
            // up with the rename emitted earlier.
            let effective_column = match renamed_to {
                Some(new_name) => new_name,
                None => *column_name,
            };
            let mut with_effective_name = (*prev_column).clone();
            with_effective_name.name = effective_column.to_owned();
            alter_columns.extend(column_alter_ops(
                effective_table,
                &with_effective_name,
                next_column,
            ));
        }

        let prev_indexes: BTreeSet<&str> = prev_projection
            .indexes
            .iter()
            .map(|index| index.name.as_str())
            .collect();
        let next_indexes: BTreeSet<&str> = next_projection
            .indexes
            .iter()
            .map(|index| index.name.as_str())
            .collect();

        // Check constraints (from @db_enforce).
        let prev_checks: BTreeSet<&str> =
            prev_projection.checks.iter().map(|c| c.name.as_str()).collect();
        let next_checks_by_name: BTreeMap<&str, &crate::ir::AddCheck> = next_projection
            .checks
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();
        for check in &prev_projection.checks {
            if !next_checks_by_name.contains_key(check.name.as_str()) {
                drop_checks.push(Op::DropCheck(DropCheck {
                    table: check.table.clone(),
                    column: check.column.clone(),
                    name: check.name.clone(),
                }));
            }
        }
        for (name, check) in &next_checks_by_name {
            if !prev_checks.contains(name) {
                add_checks.push(Op::AddCheck((*check).clone()));
            } else {
                // Same name on both sides — compare kinds. A kind
                // change means the bounds tightened/loosened or the
                // validator type changed; emit drop + add.
                if let Some(prev) = prev_projection
                    .checks
                    .iter()
                    .find(|c| c.name.as_str() == *name)
                {
                    if prev.kind != check.kind {
                        drop_checks.push(Op::DropCheck(DropCheck {
                            table: prev.table.clone(),
                            column: prev.column.clone(),
                            name: prev.name.clone(),
                        }));
                        add_checks.push(Op::AddCheck((*check).clone()));
                    }
                }
            }
        }

        for index in &prev_projection.indexes {
            if !next_indexes.contains(index.name.as_str()) {
                drop_indexes.push(Op::DropIndex(DropIndex {
                    name: index.name.clone(),
                    table: index.table.clone(),
                }));
            }
        }
        for index in &next_projection.indexes {
            if !prev_indexes.contains(index.name.as_str()) {
                add_indexes.push(Op::AddIndex(index.clone()));
            }
        }
    }

    let mut ops = Vec::new();
    // Enum creates first so tables can reference them.
    ops.append(&mut create_enums);
    ops.append(&mut alter_enums);
    // Renames before table-level changes so subsequent ops can
    // reference the new names.
    ops.append(&mut rename_tables);
    ops.append(&mut rename_columns);
    // Drop CHECK constraints before drops on the columns they protect.
    ops.append(&mut drop_checks);
    ops.append(&mut drop_indexes);
    ops.append(&mut drop_columns);
    ops.append(&mut drop_tables);
    ops.append(&mut create_tables);
    ops.append(&mut add_columns);
    ops.append(&mut alter_columns);
    ops.append(&mut add_indexes);
    // Add CHECK constraints after the columns they protect exist.
    ops.append(&mut add_checks);
    // Enum drops last — after any tables that depended on them.
    ops.append(&mut drop_enums);
    ops
}

/// Compare a column's previous and next definitions and emit the
/// alter ops required to bring `prev` into `next` shape. Order
/// inside the returned vector is intentional: type changes before
/// nullability before defaults, so each subsequent op can rely on
/// the previous one having landed.
fn column_alter_ops(table: &str, prev: &Column, next: &Column) -> Vec<Op> {
    let mut ops = Vec::new();

    if prev.ty != next.ty || prev.arity != next.arity {
        // Only emit AlterColumnType when the *type itself* or the
        // list-vs-scalar shape changes. A pure Required ↔ Optional
        // flip is handled by AlterColumnNullability below.
        let type_changed = prev.ty != next.ty;
        let shape_changed = matches!(prev.arity, crate::ir::ColumnArity::List)
            != matches!(next.arity, crate::ir::ColumnArity::List);
        if type_changed || shape_changed {
            ops.push(Op::AlterColumnType(AlterColumnType {
                table: table.to_owned(),
                column: prev.name.clone(),
                from: prev.ty.clone(),
                from_arity: prev.arity,
                to: next.ty.clone(),
                to_arity: next.arity,
            }));
        }
    }

    if prev.arity != next.arity {
        ops.push(Op::AlterColumnNullability(AlterColumnNullability {
            table: table.to_owned(),
            column: prev.name.clone(),
            from: prev.arity,
            to: next.arity,
        }));
    }

    if prev.default != next.default {
        ops.push(Op::AlterColumnDefault(AlterColumnDefault {
            table: table.to_owned(),
            column: prev.name.clone(),
            from: prev.default.clone(),
            to: next.default.clone(),
        }));
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_core::Schema;
    use cratestack_parser::parse_schema;

    fn schema(source: &str) -> Schema {
        parse_schema(source).expect("schema should parse")
    }

    const DATASOURCE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}
"#;

    fn with_models(models: &str) -> String {
        format!("{DATASOURCE}{models}")
    }

    #[test]
    fn empty_to_empty_produces_no_ops() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(""));
        assert!(diff(&prev, &next).is_empty());
    }

    #[test]
    fn adding_a_model_emits_create_table() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  balance Int
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::CreateTable(create) => {
                assert_eq!(create.name, "accounts");
                assert_eq!(create.columns.len(), 2);
                assert_eq!(create.columns[0].name, "id");
                assert!(create.columns[0].primary_key);
                assert_eq!(create.columns[1].name, "balance");
                assert!(!create.columns[1].primary_key);
            }
            other => panic!("expected CreateTable, got {other:?}"),
        }
    }

    #[test]
    fn removing_a_model_emits_drop_table() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(""));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::DropTable(drop) if drop.name == "accounts"));
        assert!(matches!(
            ops[0].destructiveness(),
            crate::ir::Destructiveness::Lossy
        ));
    }

    #[test]
    fn adding_a_column_emits_add_column() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  balance Int?
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::AddColumn(add) => {
                assert_eq!(add.table, "accounts");
                assert_eq!(add.column.name, "balance");
                assert!(matches!(add.column.arity, crate::ir::ColumnArity::Optional));
            }
            other => panic!("expected AddColumn, got {other:?}"),
        }
        assert!(matches!(
            ops[0].destructiveness(),
            crate::ir::Destructiveness::Safe
        ));
    }

    #[test]
    fn adding_required_column_without_default_is_blocking() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            ops[0].destructiveness(),
            crate::ir::Destructiveness::Blocking
        ));
    }

    #[test]
    fn adding_required_column_with_default_is_safe() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String @default('pending')
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            ops[0].destructiveness(),
            crate::ir::Destructiveness::Safe
        ));
    }

    #[test]
    fn removing_a_column_emits_drop_column() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  legacy String?
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::DropColumn(drop)
            if drop.table == "accounts" && drop.column == "legacy"));
        assert!(matches!(
            ops[0].destructiveness(),
            crate::ir::Destructiveness::Lossy
        ));
    }

    #[test]
    fn unique_field_emits_unique_index_on_create() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model User {
  id Int @id
  email String @unique
}
"#,
        ));
        let ops = diff(&prev, &next);
        // CreateTable + AddIndex
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], Op::CreateTable(_)));
        match &ops[1] {
            Op::AddIndex(index) => {
                assert_eq!(index.name, "users_email_key");
                assert_eq!(index.table, "users");
                assert_eq!(index.columns, vec!["email".to_owned()]);
                assert!(index.unique);
            }
            other => panic!("expected AddIndex, got {other:?}"),
        }
    }

    #[test]
    fn dropping_unique_emits_drop_index() {
        let prev = schema(&with_models(
            r#"
model User {
  id Int @id
  email String @unique
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model User {
  id Int @id
  email String
}
"#,
        ));
        let ops = diff(&prev, &next);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::DropIndex(drop)
            if drop.name == "users_email_key" && drop.table == "users"));
    }

    #[test]
    fn ops_are_ordered_drops_before_creates_indexes_last() {
        let prev = schema(&with_models(
            r#"
model Old {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model New {
  id Int @id
  email String @unique
}
"#,
        ));
        let ops = diff(&prev, &next);
        // Order: DropTable(old), CreateTable(new), AddIndex(new.email)
        assert_eq!(ops.len(), 3);
        assert!(matches!(&ops[0], Op::DropTable(_)));
        assert!(matches!(&ops[1], Op::CreateTable(_)));
        assert!(matches!(&ops[2], Op::AddIndex(_)));
    }

    #[test]
    fn primary_key_required_column_is_safe_to_add() {
        // Adding an `@id` column with no default would otherwise be
        // blocking, but tables created in the same migration imply
        // the column lands at table-create time, not as an alter.
        // This test exercises the *column-level* destructiveness
        // call directly to confirm PK acts as a backfill source.
        use crate::ir::{Column, ColumnArity, ColumnType, Destructiveness};
        let column = Column {
            name: "id".to_owned(),
            ty: ColumnType::Scalar("Int".to_owned()),
            arity: ColumnArity::Required,
            default: None,
            primary_key: true,
        };
        assert!(matches!(
            column.destructiveness_on_add(),
            Destructiveness::Safe
        ));
    }
}
