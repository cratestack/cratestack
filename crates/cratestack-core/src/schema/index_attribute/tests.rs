use super::parse_index_attribute;

#[test]
fn parses_bare_field_list() {
    let parsed = parse_index_attribute("@@index([email])").unwrap();
    assert_eq!(parsed.fields, vec!["email".to_string()]);
    assert_eq!(parsed.using, None);
    assert_eq!(parsed.opclass, None);
    assert_eq!(parsed.where_predicate, None);
}

#[test]
fn parses_multi_field_bare_list() {
    let parsed = parse_index_attribute("@@index([tenantId, name])").unwrap();
    assert_eq!(
        parsed.fields,
        vec!["tenantId".to_string(), "name".to_string()]
    );
}

#[test]
fn parses_using_only() {
    let parsed = parse_index_attribute("@@index([body], using: gin)").unwrap();
    assert_eq!(parsed.fields, vec!["body".to_string()]);
    assert_eq!(parsed.using, Some("gin".to_string()));
    assert_eq!(parsed.opclass, None);
}

#[test]
fn parses_using_and_opclass() {
    let parsed =
        parse_index_attribute("@@index([embedding], using: ivfflat, opclass: \"vector_l2_ops\")")
            .unwrap();
    assert_eq!(parsed.fields, vec!["embedding".to_string()]);
    assert_eq!(parsed.using, Some("ivfflat".to_string()));
    assert_eq!(parsed.opclass, Some("vector_l2_ops".to_string()));
}

#[test]
fn parses_where_predicate() {
    let parsed = parse_index_attribute("@@index([status], where: \"status = 'active'\")").unwrap();
    assert_eq!(parsed.fields, vec!["status".to_string()]);
    assert_eq!(
        parsed.where_predicate,
        Some("status = 'active'".to_string())
    );
}

#[test]
fn parses_using_opclass_and_where_together() {
    let parsed = parse_index_attribute(
        "@@index([embedding], using: ivfflat, opclass: \"vector_l2_ops\", where: \"embedding IS NOT NULL\")",
    )
    .unwrap();
    assert_eq!(parsed.using, Some("ivfflat".to_string()));
    assert_eq!(parsed.opclass, Some("vector_l2_ops".to_string()));
    assert_eq!(
        parsed.where_predicate,
        Some("embedding IS NOT NULL".to_string())
    );
}

#[test]
fn rejects_unquoted_where_value() {
    let error = parse_index_attribute("@@index([status], where: status = 'active')").unwrap_err();
    assert!(
        error.contains("expected a quoted SQL predicate"),
        "error: {error}"
    );
}

#[test]
fn rejects_empty_where_value() {
    let error = parse_index_attribute("@@index([status], where: \"\")").unwrap_err();
    assert!(error.contains("empty `where` predicate"), "error: {error}");
}

#[test]
fn rejects_duplicate_where_key() {
    let error = parse_index_attribute("@@index([status], where: \"a\", where: \"b\")").unwrap_err();
    assert!(error.contains("more than once"), "error: {error}");
}

#[test]
fn rejects_missing_brackets() {
    let error = parse_index_attribute("@@index(email)").unwrap_err();
    assert!(error.contains("must list fields as"), "error: {error}");
}

#[test]
fn rejects_empty_field_list() {
    let error = parse_index_attribute("@@index([])").unwrap_err();
    assert!(error.contains("at least one field"), "error: {error}");
}

#[test]
fn rejects_duplicate_field() {
    let error = parse_index_attribute("@@index([name, name])").unwrap_err();
    assert!(error.contains("more than once"), "error: {error}");
}

#[test]
fn rejects_invalid_field_name() {
    let error = parse_index_attribute("@@index([tenant-id])").unwrap_err();
    assert!(error.contains("invalid field name"), "error: {error}");
}

#[test]
fn rejects_unsupported_key() {
    let error = parse_index_attribute("@@index([email], sorted: true)").unwrap_err();
    assert!(error.contains("unsupported key"), "error: {error}");
}

#[test]
fn rejects_unquoted_opclass() {
    let error =
        parse_index_attribute("@@index([embedding], using: ivfflat, opclass: vector_l2_ops)")
            .unwrap_err();
    assert!(error.contains("expected a quoted string"), "error: {error}");
}

#[test]
fn rejects_invalid_using_identifier() {
    let error = parse_index_attribute("@@index([embedding], using: \"ivfflat\")").unwrap_err();
    assert!(error.contains("invalid `using` value"), "error: {error}");
}

#[test]
fn rejects_duplicate_using_key() {
    let error =
        parse_index_attribute("@@index([embedding], using: ivfflat, using: hnsw)").unwrap_err();
    assert!(error.contains("more than once"), "error: {error}");
}

#[test]
fn rejects_other_attributes() {
    let error = parse_index_attribute("@@unique([a, b])").unwrap_err();
    assert!(
        error.contains("unsupported index attribute"),
        "error: {error}"
    );
}
