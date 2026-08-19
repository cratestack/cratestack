#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_readonly_and_server_only_field_attributes() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly
  internalScore Int @server_only
}
"#,
    )
    .expect("schema with field-policy attributes should parse");

    let fields = &schema.models[0].fields;
    assert!(
        fields[1].attributes.iter().any(|a| a.raw == "@readonly"),
        "expected @readonly on balance",
    );
    assert!(
        fields[2].attributes.iter().any(|a| a.raw == "@server_only"),
        "expected @server_only on internalScore",
    );
}

#[test]
fn rejects_readonly_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @readonly
}
"#,
    )
    .expect_err("@readonly on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @readonly"),
        "error: {error}",
    );
}

#[test]
fn rejects_server_only_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @server_only
}
"#,
    )
    .expect_err("@server_only on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @server_only"),
        "error: {error}",
    );
}

#[test]
fn rejects_readonly_and_server_only_together() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly @server_only
}
"#,
    )
    .expect_err("combining @readonly + @server_only should fail");

    assert!(
        error
            .to_string()
            .contains("declares both @readonly and @server_only"),
        "error: {error}",
    );
}

#[test]
fn accepts_bare_dbgenerated_default() {
    let schema = parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated())
  createdAt DateTime @default(dbgenerated())
}
"#,
    )
    .expect("bare @default(dbgenerated()) should parse");

    let fields = &schema.models[0].fields;
    assert!(
        fields[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@default(dbgenerated())"),
    );
}

#[test]
fn rejects_dbgenerated_with_argument() {
    let error = parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated("gen_random_uuid()"))
}
"#,
    )
    .expect_err("dbgenerated() with an argument should fail");

    assert!(
        error.to_string().contains("takes no argument"),
        "error: {error}",
    );
}

#[test]
fn accepts_pii_and_sensitive_field_attributes() {
    let schema = parse_schema(
        r#"
model Customer {
  id Int @id
  email String @pii
  riskScore Int @sensitive
}
"#,
    )
    .expect("schema with @pii and @sensitive should parse");

    let fields = &schema.models[0].fields;
    assert!(fields[1].attributes.iter().any(|a| a.raw == "@pii"));
    assert!(fields[2].attributes.iter().any(|a| a.raw == "@sensitive"));
}

/// gRPC/protobuf support was removed (v0.9 breaking change), and with it
/// the `@pb(N)` shape validator (duplicate check, non-negative-integer
/// check, reserved-range check).
///
/// Deleting that validator alone would have made `@pb(N)` *inert* rather
/// than invalid: `.cstack` attributes parse generically into opaque
/// `Attribute { raw, span }` (see `crate::parse::fields`) and there is no
/// blanket "reject unknown attribute" pass, so an unrecognised name just
/// falls through (see `validate_validator_attributes`'s `_ => {}` arm).
/// That is the right default for an attribute that never existed and the
/// wrong one for an attribute that did — a v0.8 schema full of `@pb` pins
/// would keep parsing while silently meaning nothing.
///
/// So `@pb` is rejected by name in `validate::removed_attributes`. This
/// pins the user-visible half of that: the attribute is a hard error, and
/// the message says what happened rather than just "unknown".
#[test]
fn pb_field_attribute_is_rejected_as_removed() {
    let err = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb(3)
}
"#,
    )
    .expect_err("`@pb` was removed in v0.9 and must not parse as a silent no-op");

    assert!(err.to_string().contains("@pb"), "error: {err}");
    assert!(err.to_string().contains("removed in v0.9"), "error: {err}");
}

/// The rejection is wired at *every* field-bearing declaration, not just on
/// models — a `@pb` pin was equally writable on a mixin, a type, a view, or
/// the auth block in v0.8, and must fail equally loudly on all of them now.
///
/// This covers all five call sites deliberately. An earlier revision wired
/// only three (model/mixin/type) and left `view` and `auth` silently
/// accepting `@pb`, which is precisely the no-op behaviour
/// `validate::removed_attributes` exists to prevent.
#[test]
fn pb_field_attribute_is_rejected_on_every_field_bearing_declaration() {
    for (kind, source) in [
        (
            "mixin",
            r#"
mixin Timestamps {
  created_at DateTime @pb(1)
}
"#,
        ),
        (
            "type",
            r#"
type Address {
  city String @pb(1)
}
"#,
        ),
        (
            "view",
            r#"
model Widget {
  id Int @id
  name String
}

view WidgetSummary from Widget {
  id Int @id
  name String @pb(4)
  @@sql("SELECT id, name FROM widget")
}
"#,
        ),
        (
            "auth block",
            r#"
auth User {
  id String @id @pb(9)
}
"#,
        ),
    ] {
        let err =
            parse_schema(source).expect_err(&format!("`@pb` must be rejected on {kind} fields"));
        assert!(err.to_string().contains("@pb"), "{kind}: {err}");
        assert!(
            err.to_string().contains("removed in v0.9"),
            "{kind} must get the same removal guidance models get, not a bare \
             unknown-attribute error: {err}",
        );
    }
}

/// The match is on `@pb` exactly and `@pb(...)`, not on a `@pb` *prefix* —
/// an unrelated attribute that merely starts with those characters must
/// still fall through the generic unrecognised-attribute path untouched.
#[test]
fn removed_attribute_matching_does_not_swallow_longer_names() {
    parse_schema(
        r#"
model User {
  id Int @id
  secret String @pbkdf2_rounds(10)
}
"#,
    )
    .expect("only `@pb` itself and `@pb(...)` were removed");
}
