use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn db_enforce_range_emits_check_constraint() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0, max: 1000000) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.has_blocking,
        "AddCheck is conservatively Blocking"
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

#[test]
fn db_enforce_iso4217_uses_regex_predicate() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  currency String @iso4217 @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains("CHECK (currency ~ '^[A-Z]{3}$')"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn db_enforce_length_emits_length_predicate() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  email String @length(min: 3, max: 254) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("CHECK (length(email) BETWEEN 3 AND 254)"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn removing_db_enforce_drops_constraint() {
    let prev = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0) @db_enforce
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_blocking);
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TABLE members DROP CONSTRAINT members_amount_range_check;"),
        "up was: {}",
        migration.up
    );
}
