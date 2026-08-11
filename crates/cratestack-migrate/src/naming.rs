//! Shared name conventions: how `.cstack` identifiers map to SQL
//! identifiers. Kept in one place so the IR, the emitters, and the
//! verification step all agree.

/// `Customer` → `customers`. Matches the convention the macro codegen
/// uses for `ModelDescriptor::TABLE_NAME` so generated migrations
/// produce the same table names the runtime queries against.
pub fn table_name(model: &str) -> String {
    cratestack_core::route_naming::pluralize(&to_snake_case(model))
}

/// `orderCount` → `order_count`. Matches the macro codegen's column
/// naming for the same reason.
pub fn column_name(field: &str) -> String {
    to_snake_case(field)
}

/// `<table>_<column>_key` — Postgres's own convention for unique
/// constraints, and the name we use for `@unique`-implied indexes
/// across both backends so the diff is stable. A model-level
/// `@@unique([a, b])` extends the same convention across every listed
/// column in declaration order: `<table>_<a>_<b>_key`.
pub fn index_name_unique(table: &str, columns: &[&str]) -> String {
    format!("{table}_{}_key", columns.join("_"))
}

/// `<table>_<column>_idx` — name for a general (non-unique)
/// `@@index([...])` index (issue #156). When `using` names a non-default
/// access method (e.g. `ivfflat`), it's folded into the name
/// (`<table>_<column>_<using>_idx`) so a bare `@@index([field])` and a
/// specialized `@@index([field], using: ivfflat, ...)` over the exact
/// same column don't collide on the generated name — both are legitimate
/// to declare side by side (e.g. a default btree index plus an ANN
/// index).
pub fn index_name(table: &str, columns: &[&str], using: Option<&str>) -> String {
    match using {
        Some(using) => format!("{table}_{}_{using}_idx", columns.join("_")),
        None => format!("{table}_{}_idx", columns.join("_")),
    }
}

/// `<table>_<column>_<validator>_check` — stable, predictable name
/// for CHECK constraints emitted via `@db_enforce`. Predictability
/// matters because hand-written `up.pre.sql` halves may reference
/// these by name.
pub fn check_name(table: &str, column: &str, validator: &str) -> String {
    format!("{table}_{column}_{validator}_check")
}

/// `<table>_<column>_fkey` — matches Postgres's own auto-generated
/// name for a single-column foreign key constraint, so the diff
/// engine's name-based add/drop matching agrees with what Postgres
/// itself would call the constraint.
pub fn fk_name(table: &str, column: &str) -> String {
    format!("{table}_{column}_fkey")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_name_pluralises_and_snake_cases() {
        assert_eq!(table_name("Customer"), "customers");
        assert_eq!(table_name("OrderItem"), "order_items");
        assert_eq!(table_name("Address"), "addresses");
    }

    /// cratestack#504: consonant + `y` -> `ies`, not the old naive `+s`
    /// (`webhook_deliverys`). This is the exact production defect the
    /// issue reports — a hand-written migration for the grammatically
    /// correct `webhook_deliveries` table wouldn't match the old
    /// generated name.
    #[test]
    fn table_name_pluralizes_consonant_plus_y_as_ies() {
        assert_eq!(table_name("Category"), "categories");
        assert_eq!(table_name("WebhookDelivery"), "webhook_deliveries");
    }

    /// cratestack#504: vowel + `y` still gets a plain `s`, not `ies`.
    #[test]
    fn table_name_pluralizes_vowel_plus_y_as_plain_s() {
        assert_eq!(table_name("Day"), "days");
    }

    /// Proves the consolidation from cratestack#504 is real — i.e. that
    /// `table_name` here and `cratestack_core::route_naming` are calling
    /// the *same* pluralizer, not two copies that happen to agree today
    /// and can silently drift apart again tomorrow. `table_name` is
    /// exactly `pluralize(to_snake_case(model))`, the same composition
    /// `model_route_segment` performs, so for any model name the two
    /// crates' public entry points must produce byte-identical output.
    #[test]
    fn table_name_agrees_with_core_route_naming_for_every_shape() {
        for model in [
            "Customer",
            "OrderItem",
            "Address",
            "Category",
            "WebhookDelivery",
            "Day",
            "Bus",
            "Class",
            "Entry",
        ] {
            assert_eq!(
                table_name(model),
                cratestack_core::route_naming::model_route_segment(model),
                "table_name({model:?}) should agree with \
                 cratestack_core::route_naming::model_route_segment({model:?})"
            );
        }
    }

    #[test]
    fn column_name_snake_cases_only() {
        assert_eq!(column_name("orderCount"), "order_count");
        assert_eq!(column_name("id"), "id");
        assert_eq!(column_name("Email"), "email");
    }

    #[test]
    fn unique_index_name_is_stable() {
        assert_eq!(
            index_name_unique("customers", &["email"]),
            "customers_email_key"
        );
    }

    #[test]
    fn composite_unique_index_name_joins_columns_in_order() {
        assert_eq!(
            index_name_unique("applications", &["tenant_id", "name", "environment"]),
            "applications_tenant_id_name_environment_key"
        );
        // Order is significant: it is the index's column order too.
        assert_eq!(
            index_name_unique("applications", &["name", "tenant_id"]),
            "applications_name_tenant_id_key"
        );
    }

    #[test]
    fn index_name_is_stable_without_using() {
        assert_eq!(
            index_name("orders", &["customer_email"], None),
            "orders_customer_email_idx"
        );
    }

    #[test]
    fn index_name_folds_in_using_to_avoid_collision() {
        assert_eq!(
            index_name("documents", &["embedding"], Some("ivfflat")),
            "documents_embedding_ivfflat_idx"
        );
        assert_ne!(
            index_name("documents", &["embedding"], None),
            index_name("documents", &["embedding"], Some("ivfflat")),
        );
    }

    #[test]
    fn fk_name_matches_postgres_convention() {
        assert_eq!(
            fk_name("applications", "tenant_id"),
            "applications_tenant_id_fkey"
        );
    }
}
