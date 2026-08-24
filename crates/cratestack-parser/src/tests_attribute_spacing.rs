#![cfg(test)]

//! An attribute's argument group (`(...)` / `[...]`) separated from its
//! attribute name by whitespace, e.g. `@computed (params: ProxyParams?)`,
//! used to silently parse as bare `@computed` — the group was dropped on
//! the floor with no diagnostic (`parse::attribute_spacing`'s module doc
//! has the mechanism). This file pins the fix: the shape is now a hard,
//! spanned parse error instead of a silent no-op.

use super::parse_schema;

#[test]
fn rejects_computed_params_separated_by_space_on_a_type_field() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

type Thumbnail {
  storageKey String
  url String @computed (params: ProxyParams?)
}
"#,
    )
    .expect_err("`@computed (params: ProxyParams?)` must not silently drop the params group");

    let message = error.to_string();
    assert!(
        message.contains("url"),
        "error should name the field: {message}"
    );
    assert!(
        message.contains("write `@computed(params: ProxyParams?)`"),
        "error should show the fix: {message}"
    );
    assert!(
        message.contains("not `@computed (params: ProxyParams?)`"),
        "error should show the mistake: {message}"
    );
}

#[test]
fn rejects_computed_params_separated_by_space_on_a_model_field() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed (params: ProxyParams?)
}
"#,
    )
    .expect_err("`@computed (params: ProxyParams?)` must not silently drop the params group");

    let message = error.to_string();
    assert!(
        message.contains("proxyUrl"),
        "error should name the field: {message}"
    );
    assert!(
        message.contains("write `@computed(params: ProxyParams?)`"),
        "error should show the fix: {message}"
    );
}

/// Proves the fix is general, not `@computed`-specific: any attribute's
/// argument group separated by whitespace is rejected.
#[test]
fn rejects_default_args_separated_by_space() {
    let error = parse_schema(
        r#"
model Widget {
  id Int @id
  count Int @default (5)
}
"#,
    )
    .expect_err("`@default (5)` must not silently drop the argument");

    let message = error.to_string();
    assert!(message.contains("count"), "error: {message}");
    assert!(
        message.contains("write `@default(5)`"),
        "error should show the fix: {message}"
    );
    assert!(
        message.contains("not `@default (5)`"),
        "error should show the mistake: {message}"
    );
}

/// The `[` group opener must be caught too, not just `(`.
#[test]
fn rejects_relation_args_separated_by_space() {
    let error = parse_schema(
        r#"
model Author {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author Author @relation (fields: [authorId], references: [id])
}
"#,
    )
    .expect_err("`@relation (fields: ...)` must not silently drop the argument group");

    let message = error.to_string();
    assert!(message.contains("author"), "error: {message}");
    assert!(
        message.contains("write `@relation(fields: [authorId], references: [id])`"),
        "error should show the fix: {message}"
    );
}

/// Tab-separated must be caught, not just a single ASCII space.
#[test]
fn rejects_tab_separated_args() {
    let error = parse_schema("model Widget {\n  id Int @id\n  count Int @default\t(5)\n}\n")
        .expect_err("`@default\\t(5)` must not silently drop the argument");

    assert!(
        error.to_string().contains("write `@default(5)`"),
        "error: {error}"
    );
}

/// Multiple spaces must be caught too.
#[test]
fn rejects_multi_space_separated_args() {
    let error = parse_schema(
        r#"
model Widget {
  id Int @id
  count Int @default   (5)
}
"#,
    )
    .expect_err("`@default   (5)` must not silently drop the argument");

    assert!(
        error.to_string().contains("write `@default(5)`"),
        "error: {error}"
    );
}

/// A group with no preceding attribute at all is out of scope for this
/// fix — it keeps today's behaviour of being silently dropped, and no new
/// diagnostic is introduced for it. See `parse::attribute_spacing`'s module
/// doc for why.
#[test]
fn stray_group_with_no_preceding_attribute_stays_silently_dropped() {
    let schema = parse_schema(
        r#"
model Widget {
  id Int @id
  name String (foo)
}
"#,
    )
    .expect("a stray group with no preceding attribute is out of scope and must not error");

    assert!(
        schema.models[0].fields[1].attributes.is_empty(),
        "the stray group must still be silently dropped, unchanged from today's behaviour"
    );
}

/// Correctly-attached attribute arguments must keep parsing exactly as
/// today, including attributes with no whitespace at all between the name
/// and its group, and internal whitespace inside the parens (which is at
/// `depth > 0`, not the whitespace this fix targets).
#[test]
fn attached_attribute_arguments_are_unaffected() {
    let schema = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Author {
  id Int @id
}

model Post {
  id Int @id @unique
  authorId Int
  author Author @relation(fields: [authorId], references: [id])
  createdAt DateTime @default(now())
  legacyId String @default(dbgenerated())
  thumbnail String @computed(params: ProxyParams?)
}
"#,
    )
    .expect("correctly-attached attribute arguments must keep parsing");

    let fields = &schema.models[1].fields;
    assert_eq!(fields[0].attributes[0].raw, "@id");
    assert_eq!(fields[0].attributes[1].raw, "@unique");
    assert_eq!(
        fields[2].attributes[0].raw,
        "@relation(fields: [authorId], references: [id])"
    );
    assert_eq!(fields[3].attributes[0].raw, "@default(now())");
    assert_eq!(fields[4].attributes[0].raw, "@default(dbgenerated())");
    assert_eq!(
        fields[5].attributes[0].raw,
        "@computed(params: ProxyParams?)"
    );
}

/// Trailing whitespace after an attribute with nothing following it must
/// stay valid — there is no group to drop, so nothing should be rejected.
#[test]
fn trailing_whitespace_only_stays_valid() {
    let schema = parse_schema("model Widget {\n  id Int @id  \n}\n")
        .expect("trailing whitespace after an attribute with no following group must stay valid");

    assert_eq!(schema.models[0].fields[0].attributes[0].raw, "@id");
}

/// The diagnostic must carry a real span pointing at the offending
/// argument group, not just a bare message.
#[test]
fn rejected_group_has_a_span_pointing_at_the_argument_group() {
    let source = r#"
type ProxyParams {
  width Int?
}

type Thumbnail {
  storageKey String
  url String @computed (params: ProxyParams?)
}
"#;

    let error = parse_schema(source)
        .expect_err("`@computed (params: ProxyParams?)` must be rejected with a spanned error");

    let span = error.span();
    assert!(
        span.start < span.end && span.end <= source.len(),
        "span should be a non-empty range within the source: {span:?}"
    );
    let spanned_text = &source[span];
    assert_eq!(
        spanned_text, "(params: ProxyParams?)",
        "span should point at the offending argument group"
    );
}
