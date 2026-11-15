#![cfg(test)]
//! `@@map("<table>")` and `@map("<column>")` (cratestack#1375): what is
//! accepted, which forms and names are refused, and where a marker is inert.
//! Collisions between final names are in `tests_storage_map_collisions`.

use crate::parse_schema;

#[path = "tests_storage_map_collisions.rs"]
mod collisions;

const HEAD: &str =
    "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n";

fn model(body: &str) -> String {
    format!("{HEAD}model Bank {{\n  id Int @id\n  {body}\n}}\n")
}

#[track_caller]
fn accepted(source: &str) -> cratestack_core::Schema {
    parse_schema(source).unwrap_or_else(|error| panic!("should parse: {error}\n{source}"))
}

#[track_caller]
fn refused(source: &str, needles: &[&str]) {
    let message = parse_schema(source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{source}"))
        .to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
}

#[test]
fn a_model_a_field_and_a_mixin_field_take_a_map() {
    let schema = accepted(&model(
        "name String @map(\"bank_name\")\n  @@map(\"banks_v2\")",
    ));
    let bank = &schema.models[0];
    assert_eq!(bank.sql_table_name(), "banks_v2");
    assert_eq!(bank.sql_column_of("name").as_deref(), Some("bank_name"));
    assert_eq!(bank.sql_column_of("id").as_deref(), Some("id"));

    let schema = accepted(&format!(
        "mixin Stamped {{\n  createdAt DateTime @map(\"created_on\")\n}}\n\n{}",
        model("@use(Stamped)")
    ));
    assert_eq!(
        schema.models[0].sql_column_of("createdAt").as_deref(),
        Some("created_on")
    );
    let unmapped = accepted(&model("name String"));
    assert_eq!(unmapped.models[0].sql_table_name(), "banks");
}

#[test]
fn a_primary_key_unique_and_pii_field_can_be_mapped() {
    accepted(&format!(
        "{HEAD}model Bank {{\n  id Int @id @map(\"bank_id\")\n  code String @unique @map(\"bank_code\")\n  \
         ssn String @pii @map(\"ssn_enc\")\n}}\n"
    ));
}

#[test]
fn a_map_in_a_form_nothing_reads_is_refused() {
    for field in [
        "@map(\"\")",
        "@map()",
        "@map(bank_name)",
        "@map('bank_name')",
        "@map(name: \"bank_name\")",
        "@map(\"a\", \"b\")",
        "@map(\"a\\\"b\")",
    ] {
        refused(&model(&format!("name String {field}")), &["`@map`"]);
    }
    refused(
        &model("name String @map"),
        &["`@map` takes exactly one argument"],
    );
    for block in [
        "@@map(\"\")",
        "@@map()",
        "@@map(banks)",
        "@@map('banks')",
        "@@map(name: \"banks\")",
        "@@map(\"a\", \"b\")",
    ] {
        refused(&model(block), &["@@map"]);
    }
    refused(&model("@@map"), &["@@map"]);
}

#[test]
fn a_name_outside_the_grammar_is_refused() {
    let long = "a".repeat(64);
    refused(
        &model(&format!("@@map(\"{long}\")")),
        &["is 64 bytes", "at most 63"],
    );
    for name in ["public.bank", "Bank", "1x", "a-b", "a b"] {
        refused(
            &model(&format!("@@map(\"{name}\")")),
            &["not a valid table name", name],
        );
        refused(
            &model(&format!("name String @map(\"{name}\")")),
            &["not a valid column name"],
        );
    }
    accepted(&model(&format!("@@map(\"{}\")", "a".repeat(63))));
    accepted(&model("@@map(\"_x1\")"));
}

#[test]
fn a_reserved_word_or_prefix_is_refused() {
    for name in ["user", "order", "group", "select", "table"] {
        refused(
            &model(&format!("@@map(\"{name}\")")),
            &["reserved SQL word"],
        );
        refused(
            &model(&format!("name String @map(\"{name}\")")),
            &["reserved SQL word"],
        );
    }
    for name in ["cratestack_x", "sqlite_x", "pg_x"] {
        refused(
            &model(&format!("@@map(\"{name}\")")),
            &["is reserved for cratestack"],
        );
    }
    // The prefixes are for tables only.
    accepted(&model("name String @map(\"pg_name\")"));
}

#[test]
fn a_second_marker_is_refused_on_the_second() {
    refused(
        &model("@@map(\"a\")\n  @@map(\"b\")"),
        &["second `@@map`", "@@map(\"b\")"],
    );
    refused(
        &model("name String @map(\"a\") @map(\"b\")"),
        &["second `@map`", "@map(\"b\")"],
    );
}

#[test]
fn a_map_where_nothing_reads_it_is_refused() {
    let related = format!(
        "{HEAD}model Branch {{\n  id Int @id\n  bankId Int\n  bank Bank @relation(fields: [bankId], references: [id]) {{M}}\n}}\n\n{}",
        model("branches Branch[] {M}")
    );
    // The side with `@relation`, and the back-reference with none.
    refused(
        &related.replace("{M}", "@map(\"bank_ref\")"),
        &["a relation field is not a column"],
    );
    let back = related
        .replacen("{M}", "", 1)
        .replace("{M}", "@map(\"branch_list\")");
    refused(&back, &["field `branches` on model `Bank`", "not a column"]);
    refused(
        &model("slug String @computed @map(\"slug\")"),
        &["`@computed`", "`@map(\"slug\")`"],
    );
    refused(
        &format!(
            "{HEAD}model Bank {{\n  id Int @id\n}}\n\nview V from Bank {{\n  id Int @id @map(\"i\")\n  \
             @@sql(\"SELECT id FROM banks\")\n}}\n"
        ),
        &["no effect here", "a view's columns come from its SQL"],
    );
    refused(
        &format!("{HEAD}type Money {{\n  amount Int @map(\"amount_cents\")\n}}\n"),
        &["no effect here", "the block is not a table"],
    );
    refused(
        &format!("{HEAD}auth Operator {{\n  id Int @map(\"operator_id\")\n}}\n"),
        &["no effect here", "the block is not a table"],
    );
    refused(
        &format!(
            "{HEAD}model Bank {{\n  id Int @id\n}}\n\nview V from Bank {{\n  id Int @id\n  \
             @@map(\"vs\")\n  @@sql(\"SELECT id FROM banks\")\n}}\n"
        ),
        &["unsupported attribute `@@map` on a view"],
    );
}
