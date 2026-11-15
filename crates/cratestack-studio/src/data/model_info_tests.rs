use super::*;

fn parse(text: &str) -> Schema {
    cratestack_parser::parse_schema(text).expect("schema parses")
}

/// A `BigInt` key with no arm in `pk_cast_for` is refused as an
/// unsupported key type, which silently drops the model from
/// browse, relation follow and the generated find-by-key snippet.
#[test]
fn bigint_primary_key_resolves_to_the_bigint_cast() {
    let schema = parse(
        r#"
            model Ledger {
              id BigInt @id
              amountE8 BigInt
              note String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Ledger").expect("BigInt key is supported");
    assert_eq!(info.pk_cast, PkCast::BigInt);
    let scalars: Vec<(&str, &str)> = info
        .columns
        .iter()
        .map(|c| (c.field_name, c.scalar))
        .collect();
    assert_eq!(
        scalars,
        [("id", "BigInt"), ("amountE8", "BigInt"), ("note", "String")]
    );
}

/// `@@map` / `@map` (cratestack#1375): the studio's SQL runs against the mapped
/// table and columns, the key and `@version` columns included, while the field
/// names it keys rows by stay the schema's.
#[test]
fn mapped_names_reach_the_table_columns_key_and_version() {
    let schema = parse(
        r#"
            model Bank {
              id Int @id @map("bank_id")
              displayName String @map("display_name")
              rev Int @version @map("rev_no")
              @@map("map_bank")
            }
        "#,
    );
    let (model, info) = resolve_model(&schema, "Bank").expect("resolves");
    assert_eq!(info.table, "map_bank");
    assert_eq!(info.pk_column, "bank_id");
    assert_eq!(info.pk_field_name, "id");
    let columns: Vec<(&str, &str)> = info
        .columns
        .iter()
        .map(|c| (c.field_name, c.column_name.as_str()))
        .collect();
    assert_eq!(
        columns,
        [
            ("id", "bank_id"),
            ("displayName", "display_name"),
            ("rev", "rev_no")
        ]
    );
    assert_eq!(version_column(model).as_deref(), Some("rev_no"));
}
