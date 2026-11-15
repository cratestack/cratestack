//! `@@map` / `@map` reach the generated SQL names and nothing else
//! (cratestack#1375): the descriptor, relation links and field modules carry
//! the mapped table and columns, while every Rust identifier and wire name
//! stays the model's and the field's own.

use super::descriptor::generate_model_descriptor;
use crate::relation::relation_link;
use crate::shared::test_support::render;
use cratestack_core::Schema;

const FIXTURE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model MapBank {
  id      Int    @id @map("bank_id")
  code    String @unique @map("bank_code")
  ssn     String @pii @map("ssn_enc")
  version Int    @version @map("rev")
  plain   String
  deposits MapDeposit[] @relation(fields: [id], references: [bankId])

  @@map("map_bank")
}

model MapDeposit {
  id     Int @id
  bankId Int @map("bank_ref")
  bank   MapBank @relation(fields: [bankId], references: [id])

  @@map("map_deposit")
}
"#;

fn schema() -> Schema {
    cratestack_parser::parse_schema(FIXTURE).expect("the storage-map fixture must parse")
}

fn descriptor(schema: &Schema, model: usize) -> String {
    render(
        &generate_model_descriptor(
            &schema.models[model],
            &schema.models,
            &schema.types,
            &schema.enums,
            schema.auth.as_ref(),
            false,
        )
        .expect("a mapped descriptor must generate"),
    )
}

#[test]
fn the_descriptor_carries_the_mapped_table_and_columns() {
    let schema = schema();
    let rendered = descriptor(&schema, 0);
    for expected in [
        "\"map_bank\"",
        "rust_name : \"id\" , sql_name : \"bank_id\"",
        "rust_name : \"ssn\" , sql_name : \"ssn_enc\"",
        "rust_name : \"plain\" , sql_name : \"plain\"",
        "\"bank_code\"",
        "Some (\"rev\")",
    ] {
        assert!(rendered.contains(expected), "{expected} in {rendered}");
    }
    assert!(!rendered.contains("\"map_banks\""), "{rendered}");
}

#[test]
fn a_relation_link_joins_the_mapped_tables_and_columns() {
    let schema = schema();
    let deposit = &schema.models[1];
    let link = relation_link(deposit, &deposit.fields[2], &schema.models).expect("a link");
    assert_eq!(link.parent_table, "map_deposit");
    assert_eq!(link.related_table, "map_bank");
    assert_eq!(link.parent_column, "bank_ref");
    assert_eq!(link.related_column, "bank_id");
    // The to-many side joins the same two columns the other way round.
    let bank = &schema.models[0];
    let link = relation_link(bank, &bank.fields[5], &schema.models).expect("a link");
    assert_eq!(link.parent_table, "map_bank");
    assert_eq!(link.parent_column, "bank_id");
    assert_eq!(link.related_table, "map_deposit");
    assert_eq!(link.related_column, "bank_ref");
}

#[test]
fn rust_names_and_wire_names_do_not_follow_the_mapping() {
    let schema = schema();
    let rendered = descriptor(&schema, 0);
    // The descriptor's own constant and the field names the API accepts.
    assert!(rendered.contains("MAP_BANK_MODEL"), "{rendered}");
    assert!(rendered.contains("\"ssn\""), "{rendered}");
    assert_eq!(
        cratestack_core::route_naming::model_route_segment(&schema.models[0].name),
        "map_banks"
    );
}
