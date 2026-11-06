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
