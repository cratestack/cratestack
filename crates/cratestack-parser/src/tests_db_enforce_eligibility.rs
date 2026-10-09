#![cfg(test)]
//! ADR 0019 D5 (PR A): `@db_enforce` makes the validators on its field a
//! database CHECK constraint, and `@range`, `@length` and `@iso4217` are the
//! only ones with a SQL form (`cratestack-migrate/src/convert/checks.rs`).
//! On any other field it emitted nothing and passed `check`.

use super::parse_schema;

fn model_with(field: &str) -> String {
    format!("model T {{\n  id Int @id\n  f {field}\n}}\n")
}

#[track_caller]
fn refused(field: &str) -> (String, String) {
    let source = model_with(field);
    match parse_schema(&source) {
        Ok(_) => panic!("`{field}` must be refused:\n{source}"),
        Err(error) => (error.to_string(), source[error.span()].to_owned()),
    }
}

#[test]
fn db_enforce_beside_a_validator_with_no_sql_form_is_refused() {
    for (field, validator) in [
        ("String @email @db_enforce", "`@email`"),
        ("String @db_enforce @email", "`@email`"),
        ("String @uri @db_enforce", "`@uri`"),
        ("String @regex(\"^a$\") @db_enforce", "`@regex`"),
        ("String @email @uri @db_enforce", "`@email`, `@uri`"),
    ] {
        let (message, underlined) = refused(field);
        assert!(
            message.contains("the validator") && message.contains(validator),
            "`{field}`: {message}"
        );
        assert!(message.contains("has no SQL form"), "`{field}`: {message}");
        // It names what would work, and underlines the attribute that does nothing.
        assert!(
            message.contains("`@range`, `@length` or `@iso4217`"),
            "`{field}`: {message}"
        );
        assert_eq!(underlined, "@db_enforce", "`{field}`");
    }
}

#[test]
fn a_bare_db_enforce_is_refused() {
    let (message, underlined) = refused("String @db_enforce");
    assert!(
        message.contains("field `T.f` declares `@db_enforce`")
            && message.contains("no validator on the field"),
        "{message}"
    );
    assert_eq!(underlined, "@db_enforce");
}

/// `@db_enforce` is also copied into every model that uses the mixin, so a
/// mixin field is refused where it lands.
#[test]
fn a_mixin_field_with_nothing_to_enforce_is_refused_in_the_model_that_uses_it() {
    let error = parse_schema(
        "mixin M {\n  f String @email @db_enforce\n}\nmodel T {\n  id Int @id\n  @use(M)\n}\n",
    )
    .expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("field `T.f` declares `@db_enforce`"),
        "{error}"
    );
}

#[test]
fn db_enforce_stays_accepted_beside_an_eligible_validator() {
    for field in [
        "Int @range(min: 0) @db_enforce",
        "String @length(max: 5) @db_enforce",
        "String @iso4217 @db_enforce",
        // One eligible validator is enough; `@email` simply stays application-only.
        "String @email @length(max: 80) @db_enforce",
        "String @db_enforce @iso4217 @regex(\"^[A-Z]+$\")",
    ] {
        parse_schema(&model_with(field)).unwrap_or_else(|e| panic!("`{field}` refused: {e}"));
    }
}
