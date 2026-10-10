//! `BigInt` (ADR 0019) column emission.
//!
//! The Postgres emitter's `scalar_to_postgres` ends in `_ => "TEXT"`.
//! A scalar with no arm compiles, parses and migrates, and then creates
//! a TEXT column that every `i64` bind and read fails against. These
//! tests exist so that a missing arm shows up here, not in production.

use std::collections::BTreeMap;

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn bigint_columns_emit_bigint_and_never_the_text_fallback() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Ledger {
  id BigInt @id
  amountE8 BigInt
  feeE8 BigInt?
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("id BIGINT NOT NULL"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains("amount_e8 BIGINT NOT NULL"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains("fee_e8 BIGINT"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.up.contains("fee_e8 BIGINT NOT NULL"),
        "optional BigInt must be nullable; up was: {}",
        migration.up
    );
    assert!(migration.up.contains("PRIMARY KEY (id)"));
    assert!(
        !migration.up.contains("TEXT"),
        "a BigInt column fell through to the TEXT arm; up was: {}",
        migration.up
    );
}

#[test]
fn adding_a_bigint_column_emits_alter_table_bigint() {
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
  balanceE8 BigInt
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ADD COLUMN balance_e8 BIGINT NOT NULL"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration
            .down
            .contains("ALTER TABLE accounts DROP COLUMN balance_e8;")
    );
}

#[test]
fn db_enforce_range_on_bigint_emits_the_check_constraint() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount BigInt @range(min: 0, max: 1000000) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("amount BIGINT NOT NULL"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "ALTER TABLE members ADD CONSTRAINT members_amount_range_check \
             CHECK (amount >= 0 AND amount <= 1000000);"
        ),
        "up was: {}",
        migration.up
    );
}

/// The catch-all guard. Every built-in scalar that can be a model
/// column must have an explicit, expected Postgres type; the expected
/// table is compared against the parser's own `builtin_type_names()`,
/// so a new built-in with no emitter arm fails here (it either has no
/// row, or its column renders `TEXT` while its row says otherwise).
#[test]
fn every_builtin_scalar_column_has_an_explicit_postgres_type() {
    let expected: BTreeMap<&str, &str> = BTreeMap::from([
        ("String", "TEXT"),
        ("Cuid", "TEXT"),
        ("Int", "BIGINT"),
        ("BigInt", "BIGINT"),
        ("Float", "DOUBLE PRECISION"),
        ("Boolean", "BOOLEAN"),
        ("DateTime", "TIMESTAMPTZ"),
        ("Decimal", "NUMERIC"),
        ("Json", "JSONB"),
        ("Bytes", "BYTEA"),
        ("Uuid", "UUID"),
    ]);
    // Not plain scalar columns: `Page`/`PageInput`/`FindMany` are
    // procedure-only; `Vector`/`Geography`/`Geometry` take arguments and
    // an extension declaration, and render through their own variants.
    let non_scalar = [
        "Page",
        "PageInput",
        "FindMany",
        "Vector",
        "Geography",
        "Geometry",
    ];
    let builtin: Vec<&str> = cratestack_parser::builtin_type_names()
        .iter()
        .copied()
        .filter(|name| !non_scalar.contains(name))
        .collect();
    let covered: Vec<&str> = expected.keys().copied().collect();
    let mut builtin_sorted = builtin.clone();
    builtin_sorted.sort_unstable();
    assert_eq!(
        builtin_sorted, covered,
        "builtin scalars and this table drifted: give the new scalar an explicit arm in \
         emit/postgres/columns.rs::scalar_to_postgres and a row here"
    );

    for name in builtin {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(&format!(
            "model Probe {{\n  id Int @id\n  value {name}\n}}\n"
        )));
        let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
        let want = expected[name];
        let line = format!("value {want} NOT NULL");
        assert!(
            migration.up.contains(&line),
            "`{name}` should render `{want}`; up was: {}",
            migration.up
        );
    }
}
