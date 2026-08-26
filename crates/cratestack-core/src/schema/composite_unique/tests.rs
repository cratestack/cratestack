use super::parse_composite_unique_attribute;

#[test]
fn parses_two_fields() {
    let parsed = parse_composite_unique_attribute("@@unique([tenantId, name])").unwrap();
    assert_eq!(
        parsed.fields,
        vec!["tenantId".to_string(), "name".to_string()]
    );
    assert_eq!(parsed.where_predicate, None);
}

#[test]
fn parses_three_fields_in_declared_order() {
    let parsed =
        parse_composite_unique_attribute("@@unique([tenantId, name, environment])").unwrap();
    assert_eq!(
        parsed.fields,
        vec![
            "tenantId".to_string(),
            "name".to_string(),
            "environment".to_string(),
        ]
    );
}

#[test]
fn parses_where_predicate_on_a_composite_field_list() {
    let parsed = parse_composite_unique_attribute(
        "@@unique([tenantId, idempotencyKey], where: \"idempotency_key IS NOT NULL\")",
    )
    .unwrap();
    assert_eq!(
        parsed.fields,
        vec!["tenantId".to_string(), "idempotencyKey".to_string()]
    );
    assert_eq!(
        parsed.where_predicate,
        Some("idempotency_key IS NOT NULL".to_string())
    );
}

/// The motivating case (cratestack#742, ADR 0038's deferred B3): a
/// single, genuinely-optional column that must be unique only when
/// present. Field-level `@unique` can't express `where:` at all, so
/// a single-field `@@unique([...], where: "...")` is the only way
/// to spell this — unlike a *non*-partial single-field `@@unique`,
/// which is still rejected (see `rejects_single_field` below) in
/// favor of the simpler field-level form.
#[test]
fn parses_where_predicate_on_a_single_field_list() {
    let parsed = parse_composite_unique_attribute(
        "@@unique([idempotencyKey], where: \"idempotency_key IS NOT NULL\")",
    )
    .unwrap();
    assert_eq!(parsed.fields, vec!["idempotencyKey".to_string()]);
    assert_eq!(
        parsed.where_predicate,
        Some("idempotency_key IS NOT NULL".to_string())
    );
}

#[test]
fn rejects_missing_brackets() {
    let error = parse_composite_unique_attribute("@@unique(tenantId, name)").unwrap_err();
    assert!(error.contains("must list fields as"), "error: {error}");
    assert!(
        error.contains("composite unique attribute"),
        "error: {error}"
    );
}

#[test]
fn rejects_single_field() {
    let error = parse_composite_unique_attribute("@@unique([email])").unwrap_err();
    assert!(error.contains("at least two fields"), "error: {error}");
    assert!(error.contains("field-level `@unique`"), "error: {error}");
}

/// Finding 4 (cratestack#742 post-review remediation): the arity floor
/// used to be skipped entirely — not lowered from two to one — whenever
/// `where:` was present, so `@@unique([], where: "x")` parsed as valid
/// and reached `migrate`, which would emit `CREATE UNIQUE INDEX ...
/// ON table () WHERE x` — invalid DDL Postgres rejects at apply time.
/// This must be rejected here, at parse time, matching `@@index`'s own
/// unconditional `fields.is_empty()` check.
#[test]
fn rejects_empty_field_list_even_with_where() {
    let error =
        parse_composite_unique_attribute("@@unique([], where: \"idempotency_key IS NOT NULL\")")
            .unwrap_err();
    assert!(error.contains("at least one field"), "error: {error}");
}

#[test]
fn rejects_duplicate_field() {
    let error = parse_composite_unique_attribute("@@unique([name, name])").unwrap_err();
    assert!(error.contains("more than once"), "error: {error}");
}

#[test]
fn rejects_invalid_identifier() {
    let error = parse_composite_unique_attribute("@@unique([tenant-id, name])").unwrap_err();
    assert!(error.contains("invalid field name"), "error: {error}");
}

#[test]
fn rejects_other_attributes() {
    let error = parse_composite_unique_attribute("@@id([a, b])").unwrap_err();
    assert!(error.contains("unsupported composite unique attribute"));
}

#[test]
fn rejects_unquoted_where_value() {
    let error = parse_composite_unique_attribute(
        "@@unique([tenantId, name], where: idempotency_key IS NOT NULL)",
    )
    .unwrap_err();
    assert!(
        error.contains("expected a quoted SQL predicate"),
        "error: {error}"
    );
}

#[test]
fn rejects_empty_where_value() {
    let error =
        parse_composite_unique_attribute("@@unique([tenantId, name], where: \"\")").unwrap_err();
    assert!(error.contains("empty `where` predicate"), "error: {error}");
}

#[test]
fn rejects_duplicate_where_key() {
    let error =
        parse_composite_unique_attribute("@@unique([tenantId, name], where: \"a\", where: \"b\")")
            .unwrap_err();
    assert!(error.contains("more than once"), "error: {error}");
}

#[test]
fn rejects_unsupported_key() {
    let error =
        parse_composite_unique_attribute("@@unique([tenantId, name], sorted: true)").unwrap_err();
    assert!(error.contains("unsupported key"), "error: {error}");
}
