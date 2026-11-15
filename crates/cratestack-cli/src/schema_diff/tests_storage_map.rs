#![cfg(test)]
//! `@@map` / `@map` in `cratestack diff` (cratestack#1375): storage names move
//! no route, op id or client, so changing one is never a wire break.

use super::Severity;
use super::test_support::{categories, diff};

const PLAIN: &str = "model Bank {\n  id Int @id\n  code String\n}\n";

fn mapped(table: &str, column: &str) -> String {
    format!("model Bank {{\n  id Int @id\n  code String {column}\n\n  @@map(\"{table}\")\n}}\n")
}

#[test]
fn adding_changing_or_removing_map_is_internal_only() {
    for (prev, next) in [
        (PLAIN.to_owned(), mapped("map_bank", "")),
        (mapped("map_bank", ""), mapped("bank_v2", "")),
        (mapped("map_bank", ""), PLAIN.to_owned()),
    ] {
        let result = diff(&prev, &next);
        assert!(!result.has_breaking(), "{:?}", result.changes);
        assert_eq!(
            categories(&result, Severity::Internal),
            vec!["model_attribute_other"],
            "{prev}\n->\n{next}"
        );
        assert!(categories(&result, Severity::Additive).is_empty());
    }
}

#[test]
fn adding_a_column_map_is_never_breaking() {
    let result = diff(PLAIN, &mapped("banks", "@map(\"bank_code\")"));
    assert!(!result.has_breaking(), "{:?}", result.changes);
    assert!(categories(&result, Severity::Additive).is_empty());
}
