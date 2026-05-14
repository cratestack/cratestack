//! Shared name conventions: how `.cstack` identifiers map to SQL
//! identifiers. Kept in one place so the IR, the emitters, and the
//! verification step all agree.

/// `Customer` → `customers`. Matches the convention the macro codegen
/// uses for `ModelDescriptor::TABLE_NAME` so generated migrations
/// produce the same table names the runtime queries against.
pub fn table_name(model: &str) -> String {
    pluralize(&to_snake_case(model))
}

/// `orderCount` → `order_count`. Matches the macro codegen's column
/// naming for the same reason.
pub fn column_name(field: &str) -> String {
    to_snake_case(field)
}

/// `<table>_<column>_key` — Postgres's own convention for unique
/// constraints, and the name we use for `@unique`-implied indexes
/// across both backends so the diff is stable.
pub fn index_name_unique(table: &str, column: &str) -> String {
    format!("{table}_{column}_key")
}

/// Convert PascalCase or camelCase to snake_case. Mirrors
/// `cratestack-macros::shared::to_snake_case`.
fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowercase in character.to_lowercase() {
                output.push(lowercase);
            }
        } else {
            output.push(character);
        }
    }
    output
}

/// Naive English pluralization — matches the codegen's
/// `pluralize` helper. Sufficient for table-naming since model names
/// are developer-controlled; if a developer wants a different table
/// name they will eventually be able to declare it via `@@map(...)`
/// (out of scope for this slice).
fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_name_pluralises_and_snake_cases() {
        assert_eq!(table_name("Customer"), "customers");
        assert_eq!(table_name("OrderItem"), "order_items");
        assert_eq!(table_name("Address"), "addresses");
    }

    #[test]
    fn column_name_snake_cases_only() {
        assert_eq!(column_name("orderCount"), "order_count");
        assert_eq!(column_name("id"), "id");
        assert_eq!(column_name("Email"), "email");
    }

    #[test]
    fn unique_index_name_is_stable() {
        assert_eq!(index_name_unique("customers", "email"), "customers_email_key");
    }
}
