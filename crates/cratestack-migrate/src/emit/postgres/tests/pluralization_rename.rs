//! cratestack#504's migration story specifically: the `y -> ies`
//! pluralization fix changes a model's *derived* table name
//! (`categorys` -> `categories`) without the model itself being
//! renamed. Split out from `renames.rs` (which covers `@@rename` in
//! general) to keep both files under the ~200-LoC convention.

use super::super::emit;
use super::{schema, with_models};
use crate::convert::TableProjection;
use crate::diff::diff_projections;
use crate::ir::{Column, ColumnArity, ColumnType};
use crate::projection::{Projections, project};

/// A hand-built [`Projections`] table named `categorys` — the real
/// deployed name a pre-#504 build produced for `model Category`. This
/// can't be reproduced by parsing a `.cstack` model through the
/// current (already-fixed) pluralizer, since the fix applies
/// uniformly; the old name only survives in an already-deployed
/// database, exactly the scenario finding 1 describes.
fn deployed_categorys_table() -> Projections {
    let mut prev = Projections::default();
    prev.tables.insert(
        "categorys".to_string(),
        TableProjection {
            name: "categorys".to_string(),
            rename_from: None,
            columns: vec![
                Column {
                    name: "id".to_string(),
                    ty: ColumnType::Scalar("Int".to_string()),
                    arity: ColumnArity::Required,
                    default: None,
                    primary_key: true,
                },
                Column {
                    name: "label".to_string(),
                    ty: ColumnType::Scalar("String".to_string()),
                    arity: ColumnArity::Required,
                    default: None,
                    primary_key: false,
                },
            ],
            column_renames: Vec::new(),
            indexes: Vec::new(),
            checks: Vec::new(),
            foreign_keys: Vec::new(),
        },
    );
    prev
}

/// `model Category` didn't rename, but the `y -> ies` pluralization
/// fix changes the table name it derives out from under any consumer
/// with a deployed `categorys` table. `@@rename(from = "categorys")`
/// must resolve this to a rename, not a DropTable+CreateTable that
/// destroys the table's data — proving the escape hatch this PR's
/// migration guidance depends on actually covers the "name unchanged,
/// derived table name changed" case, not just the "model renamed"
/// case `renames.rs` covers.
#[test]
fn pluralization_change_with_rename_marker_is_a_rename_not_drop_and_create() {
    let prev = deployed_categorys_table();
    let next = schema(&with_models(
        r#"
model Category {
  id Int @id
  label String

  @@rename(from = "categorys")
}
"#,
    ));
    let migration = emit(&diff_projections(&prev, &project(&next)));
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration
            .up
            .contains("ALTER TABLE categorys RENAME TO categories;"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("DROP TABLE"));
    assert!(!migration.up.contains("CREATE TABLE"));
}

/// Without the `@@rename` marker, the same pluralization-driven table
/// name change is indistinguishable from an unrelated table
/// disappearing and a new one appearing — the diff engine matches by
/// name only (see `crate::diff` module docs), so it emits
/// DropTable+CreateTable and would destroy the table's data. This is
/// exactly the destructive default behaviour the migration guidance in
/// `route_naming.rs` warns consumers to route around by declaring
/// `@@rename` before running `migrate diff`.
#[test]
fn pluralization_change_without_rename_marker_drops_and_recreates() {
    let prev = deployed_categorys_table();
    let next = schema(&with_models(
        r#"
model Category {
  id Int @id
  label String
}
"#,
    ));
    let migration = emit(&diff_projections(&prev, &project(&next)));
    assert!(migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration.up.contains("DROP TABLE categorys"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains("CREATE TABLE categories"),
        "up was: {}",
        migration.up
    );
}
