use super::*;
use crate::schema::SourceSpan;
use crate::schema::model::{Attribute, TypeArity, TypeRef};

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

fn attribute(raw: &str) -> Attribute {
    Attribute {
        raw: raw.to_owned(),
        span: span(),
    }
}

fn field(name: &str, attributes: &[&str]) -> Field {
    Field {
        docs: Vec::new(),
        name: name.to_owned(),
        name_span: span(),
        ty: TypeRef {
            name: "String".to_owned(),
            name_span: span(),
            arity: TypeArity::Required,
            generic_args: Vec::new(),
            int_args: Vec::new(),
            ident_args: Vec::new(),
        },
        attributes: attributes.iter().map(|raw| attribute(raw)).collect(),
        span: span(),
    }
}

fn model(name: &str, attributes: &[&str], fields: Vec<Field>) -> Model {
    Model {
        docs: Vec::new(),
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: attributes.iter().map(|raw| attribute(raw)).collect(),
        span: span(),
        mcp: None,
    }
}

#[test]
fn an_unmapped_model_and_field_keep_the_derived_names() {
    let m = model(
        "WebhookDelivery",
        &["@@audit"],
        vec![field("accountId", &[])],
    );
    assert_eq!(m.sql_table_name(), "webhook_deliveries");
    assert_eq!(m.sql_column_of("accountId").as_deref(), Some("account_id"));
    assert_eq!(m.sql_column_of("missing"), None);
}

#[test]
fn a_map_marker_replaces_the_derived_name() {
    let m = model(
        "Deposit",
        &["@@map(\"legacy_deposits\")"],
        vec![field("ssn", &["@pii", "@map(\"ssn_enc\")"])],
    );
    assert_eq!(m.sql_table_name(), "legacy_deposits");
    assert_eq!(m.sql_column_of("ssn").as_deref(), Some("ssn_enc"));
}

#[test]
fn only_the_exact_marker_is_read() {
    for raw in [
        "@@mapx(\"t\")",
        "@@map",
        "@@map()",
        "@@map(t)",
        "@map(\"t\")",
        "@@map(\"t\"),",
    ] {
        let m = model("Deposit", &[raw], Vec::new());
        assert_eq!(m.sql_table_name(), "deposits", "{raw}");
    }
    for raw in ["@@map(\"c\")", "@mapped(\"c\")", "@map(c)", "@map('c')"] {
        assert_eq!(field("ssn", &[raw]).sql_column_name(), "ssn", "{raw}");
    }
}

#[test]
fn the_argument_is_one_quoted_plain_name() {
    assert_eq!(parse_map_name("  \"a_b\" "), Some("a_b"));
    for bad in [
        "",
        "\"\"",
        "a",
        "'a'",
        "name: \"a\"",
        "\"a\", \"b\"",
        "\"a\\\"b\"",
        "\"a\" \"b\"",
    ] {
        assert_eq!(parse_map_name(bad), None, "{bad}");
    }
}

#[test]
fn identifiers_follow_the_grammar() {
    use StorageNameKind::{Column, Table};
    for ok in ["a", "_a", "map_bank", "t1", &"a".repeat(63)] {
        assert_eq!(validate_sql_identifier(ok, Table), Ok(()), "{ok}");
    }
    for bad in [
        "Bank",
        "1x",
        "public.bank",
        "a-b",
        "a b",
        "",
        "é",
        &"a".repeat(64),
    ] {
        assert!(validate_sql_identifier(bad, Column).is_err(), "{bad}");
    }
    assert!(
        validate_sql_identifier("user", Table)
            .unwrap_err()
            .contains("reserved")
    );
    assert!(validate_sql_identifier("order", Column).is_err());
    for prefix in ["cratestack_x", "sqlite_x", "pg_x"] {
        assert!(validate_sql_identifier(prefix, Table).is_err(), "{prefix}");
        assert_eq!(validate_sql_identifier(prefix, Column), Ok(()), "{prefix}");
    }
}

/// The mapping is storage-only: a mapped model has the REST routes, op keys and
/// verbs of the same model without it (cratestack#1375).
#[test]
fn a_mapped_model_has_the_routes_and_op_keys_of_an_unmapped_one() {
    use crate::op_list::{ModelVerb, model_list_route, model_op_key, model_verbs};

    let plain = model("Bank", &["@@paged"], vec![field("code", &[])]);
    let mapped = model(
        "Bank",
        &["@@paged", "@@map(\"map_bank\")"],
        vec![field("code", &["@map(\"bank_code\")"])],
    );
    assert_eq!(mapped.sql_table_name(), "map_bank");
    assert_eq!(plain.sql_table_name(), "banks");
    assert_eq!(model_list_route(&mapped.name), "/banks");
    assert_eq!(model_verbs(&mapped), model_verbs(&plain));
    for rpc in [false, true] {
        for verb in model_verbs(&mapped) {
            assert_eq!(
                model_op_key(&mapped.name, verb, rpc),
                model_op_key(&plain.name, verb, rpc),
                "{verb:?} rpc={rpc}"
            );
        }
    }
    assert_eq!(
        model_op_key("Bank", ModelVerb::List, true).as_deref(),
        Some("model.Bank.list")
    );
}

#[test]
fn a_column_may_not_be_a_postgres_system_column_or_a_boolean_literal() {
    for name in ["xmin", "xmax", "cmin", "cmax", "ctid", "oid", "tableoid"] {
        let error = validate_sql_identifier(name, StorageNameKind::Column).unwrap_err();
        assert!(
            error.contains(name) && error.contains("system column"),
            "{error}"
        );
    }
    for name in ["true", "false", "values"] {
        for kind in [StorageNameKind::Table, StorageNameKind::Column] {
            assert!(validate_sql_identifier(name, kind).is_err(), "{name}");
        }
    }
    // A table may be called `oid`-like names: system columns are per column.
    assert!(validate_sql_identifier("ctid", StorageNameKind::Table).is_ok());
    assert!(validate_sql_identifier("xmin_value", StorageNameKind::Column).is_ok());
}
