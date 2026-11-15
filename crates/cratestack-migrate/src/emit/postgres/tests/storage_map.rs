//! `@@map` / `@map` in emitted DDL (cratestack#1375): the table, columns,
//! constraint and index names, and the foreign key's target all use the mapped
//! names, and none uses the derived ones.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn a_mapped_table_is_created_under_the_mapped_names() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Bank {
  id Int @id @map("bank_id")
  code String @unique @map("bank_code")
  @@map("map_bank")
}

model Deposit {
  id Int @id
  bankId Int @map("bank_ref")
  memo String @map("memo_text")
  bank Bank @relation(fields:[bankId],references:[id])
  @@map("map_deposit")
  @@unique([bankId, memo])
}
"#,
    ));
    let up = emit(&diff(&prev, &next).expect("diff should succeed")).up;
    for wanted in [
        "CREATE TABLE map_bank",
        "bank_id BIGINT NOT NULL",
        "bank_code TEXT NOT NULL",
        "PRIMARY KEY (bank_id)",
        "CREATE TABLE map_deposit",
        "bank_ref BIGINT NOT NULL",
        "map_bank_bank_code_key",
        "map_deposit_bank_ref_memo_text_key",
        "REFERENCES map_bank (bank_id)",
    ] {
        assert!(up.contains(wanted), "missing `{wanted}` in:\n{up}");
    }
    for derived in [
        "banks",
        "deposits",
        "bank_id_key",
        " code ",
        "bankId",
        "memo ",
    ] {
        assert!(!up.contains(derived), "derived name `{derived}` in:\n{up}");
    }
}
