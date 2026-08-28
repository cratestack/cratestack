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

/// gRPC/protobuf support was removed (a breaking change that shipped in
/// 0.8.5), and with it the `@pb(N)` shape validator (duplicate check,
/// non-negative-integer check, reserved-range check).
///
/// Deleting that validator alone would have made `@pb(N)` *inert* rather
/// than invalid: `.cstack` attributes parse generically into opaque
/// `Attribute { raw, span }` (see `crate::parse::fields`) and there is no
/// blanket "reject unknown attribute" pass, so an unrecognised name just
/// falls through (see `validate_validator_attributes`'s `_ => {}` arm).
/// That is the right default for an attribute that never existed and the
/// wrong one for an attribute that did — a pre-0.8.5 schema full of `@pb`
/// pins would keep parsing while silently meaning nothing.
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
    .expect_err("`@pb` was removed in 0.8.5 and must not parse as a silent no-op");

    assert!(err.to_string().contains("@pb"), "error: {err}");
    assert!(err.to_string().contains("removed in 0.8.5"), "error: {err}");
}

/// The rejection is wired at *every* field-bearing declaration, not just on
/// models — a `@pb` pin was equally writable on a mixin, a type, a view, or
/// the auth block before 0.8.5, and must fail equally loudly on all of them
/// now.
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
            err.to_string().contains("removed in 0.8.5"),
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

/// cratestack#679 (half 1 of 2 — see the module doc on
/// `validate::removed_attributes` for why the typo-class half, e.g.
/// `@raedonly` silently dropping `@readonly`, is deliberately out of scope
/// here): field-level `@allow(...)` parses, is retained in the IR, and is
/// never read by any codegen — a silent no-op that looks like access
/// control and enforces nothing. It must be a hard parse error instead,
/// naming the field and pointing at the real alternatives.
#[test]
fn allow_field_attribute_is_rejected() {
    let err = parse_schema(
        r#"
model Asset {
  id Int @id
  bucket String @allow(auth().role == "system")
}
"#,
    )
    .expect_err("field-level `@allow` must not parse as a silent no-op");

    assert!(err.to_string().contains("@allow"), "error: {err}");
    assert!(err.to_string().contains("bucket"), "error: {err}");
    assert!(
        err.to_string().contains("@@allow"),
        "error should point at the model-level alternative: {err}",
    );
}

/// Same defect, `@deny` half.
#[test]
fn deny_field_attribute_is_rejected() {
    let err = parse_schema(
        r#"
model Asset {
  id Int @id
  bucket String @deny(auth().role != "system")
}
"#,
    )
    .expect_err("field-level `@deny` must not parse as a silent no-op");

    assert!(err.to_string().contains("@deny"), "error: {err}");
    assert!(
        err.to_string().contains("@@deny"),
        "error should point at the model-level alternative: {err}",
    );
}

/// Mirrors `pb_field_attribute_is_rejected_on_every_field_bearing_declaration`:
/// field-level `@allow`/`@deny` must be rejected on all five field-bearing
/// declaration kinds, not just `model`. A missed call site in
/// `validate::removed_attributes` fails *silently* (the attribute goes back
/// to being an inert no-op), so this is the guard against that.
#[test]
fn allow_and_deny_field_attributes_are_rejected_on_every_field_bearing_declaration() {
    for attribute in ["@allow(auth() != null)", "@deny(auth() == null)"] {
        for (kind, source) in [
            (
                "model",
                format!(
                    r#"
model Asset {{
  id Int @id
  bucket String {attribute}
}}
"#
                ),
            ),
            (
                "mixin",
                format!(
                    r#"
mixin Timestamps {{
  created_at DateTime {attribute}
}}
"#
                ),
            ),
            (
                "type",
                format!(
                    r#"
type Address {{
  city String {attribute}
}}
"#
                ),
            ),
            (
                "view",
                format!(
                    r#"
model Widget {{
  id Int @id
  name String
}}

view WidgetSummary from Widget {{
  id Int @id
  name String {attribute}
  @@sql("SELECT id, name FROM widget")
}}
"#
                ),
            ),
            (
                "auth block",
                format!(
                    r#"
auth User {{
  id String @id {attribute}
}}
"#
                ),
            ),
        ] {
            let err = parse_schema(&source)
                .expect_err(&format!("`{attribute}` must be rejected on {kind} fields"));
            let name = if attribute.starts_with("@allow") {
                "@allow"
            } else {
                "@deny"
            };
            assert!(err.to_string().contains(name), "{kind}: {err}");
            assert!(
                err.to_string().contains("not supported at field position"),
                "{kind} must get field-policy guidance, not a bare unknown-attribute error: {err}",
            );
        }
    }
}

/// Regression guard for cratestack#679's scope boundary: procedure-level
/// `@allow`/`@deny` is real, supported policy
/// (`cratestack-macros/src/policy/procedure.rs`) on a *procedure's*
/// attribute list, not a field's — the field-position rejection added for
/// #679 must not touch it.
#[test]
fn procedure_level_allow_still_parses() {
    parse_schema(
        r#"
auth UserAuth {
  id Int
  role String
}

type PublishPostInput {
  postId Int
}

mutation procedure publishPost(args: PublishPostInput): PublishPostInput
  @allow(auth().role == "admin")
"#,
    )
    .expect("procedure-level `@allow` must keep parsing — it is real, supported policy");
}

/// Regression guard for cratestack#679's scope boundary: model/view-level
/// `@@allow`/`@@deny` (double-`@`) is real, supported policy
/// (`cratestack-macros/src/policy/model.rs`) on the model/view's own
/// attribute list, not a field's — the field-position rejection must match
/// the bare single-`@` name precisely and not swallow the double-`@` form.
#[test]
fn model_level_double_at_allow_and_deny_still_parse() {
    parse_schema(
        r#"
auth UserAuth {
  id Int
  role String
}

model User {
  id Int @id
  email String @unique
  role String

  @@allow("read", auth() != null)
  @@deny("read", auth().role == "banned")
}
"#,
    )
    .expect("model-level `@@allow`/`@@deny` must keep parsing — they are real, supported policy");
}

/// cratestack#679's typo half. Mirrors
/// `pb_field_attribute_is_rejected_on_every_field_bearing_declaration`:
/// a missed call site in `validate::misspelled_attributes` fails
/// *silently* — the typo goes back to being an inert no-op, which is the
/// exact bug — so this covers all five field-bearing declarations.
#[test]
fn a_misspelled_field_attribute_is_rejected_on_every_field_bearing_declaration() {
    for (kind, source) in [
        (
            "model",
            r#"
model Asset {
  id Int @id
  bucket String @raedonly
}
"#,
        ),
        (
            "mixin",
            r#"
mixin Timestamps {
  created_at DateTime @raedonly
}
"#,
        ),
        (
            "type",
            r#"
type Address {
  city String @raedonly
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
  name String @raedonly
  @@sql("SELECT id, name FROM widget")
}
"#,
        ),
        (
            "auth block",
            r#"
auth User {
  id String @id
  label String @raedonly
}
"#,
        ),
    ] {
        let err = parse_schema(source)
            .expect_err(&format!("`@raedonly` must be rejected on {kind} fields"));
        let message = err.to_string();
        assert!(
            message.contains("@raedonly"),
            "{kind} must name the offending attribute: {message}"
        );
        assert!(
            message.contains("@readonly"),
            "{kind} must suggest the attribute the author meant: {message}"
        );
    }
}

/// The other half of cratestack#679's option (b), and the reason it was
/// chosen over a closed attribute set: an attribute that resembles nothing
/// is left inert rather than becoming a parse error. This is the ticket's
/// own `@totallyBogusAttribute` example.
///
/// Without this, "reject every unrecognised attribute" would satisfy the
/// test above while being a different — and much larger — language change
/// than the one that was decided.
#[test]
fn an_unknown_attribute_that_resembles_nothing_still_parses() {
    parse_schema(
        r#"
model Asset {
  id Int @id
  bucket String @totallyBogusAttribute(whatever == 1)
}
"#,
    )
    .expect("an attribute that is not a near-miss of a known name stays inert (option (b))");
}

/// Guards against the near-miss check over-rejecting real, supported
/// attributes — the failure mode that made option (a) too risky to take.
/// Every one of these must keep parsing exactly as before.
#[test]
fn supported_field_attributes_are_unaffected_by_the_near_miss_check() {
    parse_schema(
        r#"
model Widget {
  id Int @id
  code String @unique @length(min: 1, max: 10)
  email String @email
  price Decimal @range(min: 0)
  slug String @regex("^[a-z]+$")
  currency String @iso4217
  secret String @server_only
  balance Decimal @readonly
  note String @sensitive
  owner String @pii
  rev Int @version
}
"#,
    )
    .expect("every supported field attribute must survive the near-miss check");
}
