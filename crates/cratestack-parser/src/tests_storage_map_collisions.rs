#![cfg(test)]
//! Collisions between final storage names (cratestack#1375): tables across
//! models and views, columns within a model.

use crate::parse_schema;

const HEAD: &str =
    "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n";

#[track_caller]
fn refused(body: &str, needles: &[&str]) {
    let source = format!("{HEAD}{body}");
    let message = parse_schema(&source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{source}"))
        .to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
}

#[track_caller]
fn accepted(body: &str) {
    let source = format!("{HEAD}{body}");
    parse_schema(&source).unwrap_or_else(|error| panic!("should parse: {error}\n{source}"));
}

#[test]
fn two_models_may_not_share_a_mapped_table() {
    refused(
        "model A {\n  id Int @id\n  @@map(\"shared\")\n}\n\nmodel B {\n  id Int @id\n  @@map(\"shared\")\n}\n",
        &["model `B` is stored in table `shared`", "model `A`"],
    );
}

#[test]
fn a_mapped_table_may_not_equal_another_models_derived_table() {
    refused(
        "model Deposit {\n  id Int @id\n}\n\nmodel Ledger {\n  id Int @id\n  @@map(\"deposits\")\n}\n",
        &[
            "model `Ledger` is stored in table `deposits`",
            "model `Deposit`",
        ],
    );
    refused(
        "model Ledger {\n  id Int @id\n  @@map(\"deposits\")\n}\n\nmodel Deposit {\n  id Int @id\n}\n",
        &["model `Deposit` is stored in table `deposits`"],
    );
}

#[test]
fn a_mapped_table_may_not_equal_a_views_name() {
    refused(
        "model Bank {\n  id Int @id\n  @@map(\"bank_totals\")\n}\n\nview BankTotal from Bank {\n  id Int @id\n  \
         @@sql(\"SELECT id FROM bank_totals\")\n}\n",
        &["bank_totals"],
    );
}

#[test]
fn a_mapped_model_may_swap_names_with_nothing_else() {
    // Mapping a model to its own derived name, or to a free one, is fine.
    accepted("model Deposit {\n  id Int @id\n  @@map(\"deposits\")\n}\n");
    accepted(
        "model Deposit {\n  id Int @id\n  @@map(\"legacy_deposits\")\n}\n\nmodel Other {\n  id Int @id\n}\n",
    );
}

#[test]
fn a_column_may_not_be_mapped_twice() {
    refused(
        "model A {\n  id Int @id\n  a Int @map(\"x\")\n  b Int @map(\"x\")\n}\n",
        &["field `b` on model `A` collides with field `a`", "`x`"],
    );
}

#[test]
fn a_mapped_column_may_not_equal_a_siblings_derived_column() {
    refused(
        "model A {\n  id Int @id\n  accountId Int\n  other Int @map(\"account_id\")\n}\n",
        &[
            "field `other` on model `A` collides with field `accountId`",
            "`account_id`",
        ],
    );
    // Moving a field's column away frees its derived name.
    accepted(
        "model A {\n  id Int @id\n  accountId Int @map(\"acct\")\n  other Int @map(\"account_id\")\n}\n",
    );
}

#[test]
fn a_column_may_not_be_the_soft_delete_column() {
    refused(
        "model A {\n  id Int @id\n  gone DateTime? @map(\"deleted_at\")\n  @@soft_delete\n}\n",
        &["mapped to column `deleted_at`", "@@soft_delete"],
    );
    accepted("model A {\n  id Int @id\n  gone DateTime? @map(\"deleted_at\")\n}\n");
}

#[test]
fn a_mixin_column_collides_in_the_model_that_uses_it() {
    refused(
        "mixin Stamped {\n  createdAt DateTime @map(\"name\")\n}\n\nmodel A {\n  id Int @id\n  name String\n  @use(Stamped)\n}\n",
        &["collides with field", "`name`"],
    );
}

#[test]
fn the_codegen_name_guard_holds_when_a_colliding_field_is_mapped_away() {
    // `fooBar` and `foo_bar` both become the Dart/TS identifier `fooBar` and
    // the `FOO_BAR` constant whatever column they live in.
    refused(
        "model Bank {\n  id Int @id\n  fooBar String\n  foo_bar String @map(\"legacy_foo\")\n}\n",
        &["field `foo_bar` on model `Bank` collides with field `fooBar`", "`foo_bar`"],
    );
    refused(
        "model Bank {\n  id Int @id\n  fooBar String @map(\"legacy_foo\")\n  foo_bar String\n}\n",
        &["collides with field `fooBar`"],
    );
}

#[test]
fn only_stored_fields_compete_for_a_column() {
    // A relation's scalar column may be mapped to the relation's name.
    accepted(
        "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  ownerId Int @map(\"owner\")\n  \
         owner User @relation(fields:[ownerId],references:[id])\n}\n",
    );
    // A `@computed` field has no column.
    accepted(
        "model Post {\n  id Int @id\n  title String @map(\"label\")\n  \
         label String @computed\n}\n",
    );
    // Two stored fields still may not share one column.
    refused(
        "model A {\n  id Int @id\n  a Int @map(\"x\")\n  b Int @map(\"x\")\n}\n",
        &["field `b` on model `A` collides with field `a`", "`x`"],
    );
}

#[test]
fn a_soft_delete_field_may_not_be_mapped_away_from_deleted_at() {
    refused(
        "model A {\n  id Int @id\n  deletedAt DateTime? @map(\"removed_at\")\n  @@soft_delete\n}\n",
        &["field `deletedAt` on model `A`", "`deleted_at`", "@@soft_delete"],
    );
    accepted("model A {\n  id Int @id\n  deletedAt DateTime?\n  @@soft_delete\n}\n");
    accepted("model A {\n  id Int @id\n  deletedAt DateTime? @map(\"removed_at\")\n}\n");
}
