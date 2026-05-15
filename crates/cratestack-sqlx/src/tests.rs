use cratestack_sql::{FilterValue, OrderTarget};

use crate::{
    FieldRef, FilterExpr, ModelColumn, ModelDescriptor, OrderClause, PolicyExpr, ReadPolicy,
    ReadPredicate, SortDirection, SqlColumnValue, SqlValue,
    query::render_update_preview_sql,
    render::{render_filter_expr_sql, render_order_clause_sql, render_read_policy_sql},
};
use cratestack_core::{CoolContext, Value};

#[test]
fn select_projection_aliases_sql_columns_to_rust_fields() {
    let descriptor = ModelDescriptor::<(), i64>::new(
        "Post",
        "posts",
        &[
            ModelColumn {
                rust_name: "id",
                sql_name: "id",
            },
            ModelColumn {
                rust_name: "authorId",
                sql_name: "author_id",
            },
        ],
        "id",
        &["id", "authorId"],
        &["author"],
        &["id", "authorId", "author.email"],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        None,
        false,
        &[],
        &[],
        None,
        None,
        &[],
    );

    assert_eq!(
        descriptor.select_projection(),
        "id AS \"id\", author_id AS \"authorId\""
    );
}

#[test]
fn create_preview_sql_numbers_placeholders() {
    let values = [
        SqlColumnValue {
            column: "title",
            value: SqlValue::String("hello".to_owned()),
        },
        SqlColumnValue {
            column: "published",
            value: SqlValue::Bool(true),
        },
    ];

    let columns = values
        .iter()
        .map(|value| value.column)
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=values.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");

    assert_eq!(columns, "title, published");
    assert_eq!(placeholders, "$1, $2");
}

#[test]
fn field_ref_builds_filter_and_order() {
    let filter = FieldRef::<(), bool>::new("published").is_true();
    let order = FieldRef::<(), String>::new("title").desc();
    let contains = FieldRef::<(), String>::new("title").contains("hel");
    let maybe_null = FieldRef::<(), Option<String>>::new("subtitle").is_null();
    let in_filter = FieldRef::<(), i64>::new("id").in_([1_i64, 2_i64]);

    assert_eq!(filter.column, "published");
    assert_eq!(filter.value, FilterValue::Single(SqlValue::Bool(true)));
    assert_eq!(order.direction, SortDirection::Desc);
    assert!(matches!(order.target, OrderTarget::Column("title")));
    assert_eq!(
        contains.value,
        FilterValue::Single(SqlValue::String("%hel%".to_owned()))
    );
    assert_eq!(maybe_null.column, "subtitle");
    assert_eq!(
        in_filter.value,
        FilterValue::Many(vec![SqlValue::Int(1), SqlValue::Int(2)])
    );
}

#[test]
fn relation_filters_render_explicit_quantifiers() {
    let some = FilterExpr::relation_some(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), String>::new("label")
            .contains("Revoked")
            .into(),
    );
    let every = FilterExpr::relation_every(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), String>::new("label")
            .contains("Session")
            .into(),
    );
    let none = FilterExpr::relation_none(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), Option<String>>::new("revoked_at")
            .is_not_null()
            .into(),
    );
    let mut bind_index = 1usize;
    let mut some_sql = String::new();
    render_filter_expr_sql(&some, &mut some_sql, &mut bind_index);
    let mut every_sql = String::new();
    render_filter_expr_sql(&every, &mut every_sql, &mut bind_index);
    let mut none_sql = String::new();
    render_filter_expr_sql(&none, &mut none_sql, &mut bind_index);

    assert_eq!(
        some_sql,
        "EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND label LIKE $1)"
    );
    assert_eq!(
        every_sql,
        "NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND NOT (label LIKE $2))"
    );
    assert_eq!(
        none_sql,
        "NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND revoked_at IS NOT NULL)"
    );
}

#[test]
fn relation_scalar_order_preview_uses_correlated_subquery() {
    let clause = OrderClause::relation_scalar(
        "posts",
        "author_id",
        "users",
        "id",
        "users.email",
        SortDirection::Asc,
    );
    let mut sql = String::new();
    render_order_clause_sql(&clause, &mut sql);

    assert_eq!(
        sql,
        "(SELECT users.email FROM users WHERE users.id = posts.author_id LIMIT 1) ASC NULLS LAST"
    );
}

#[test]
fn relation_policy_preview_uses_exists_subquery() {
    let policy = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::Relation {
            quantifier: crate::RelationQuantifier::ToOne,
            parent_table: "posts",
            parent_column: "author_id",
            related_table: "users",
            related_column: "id",
            expr: &PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
                column: "email",
                auth_field: "email",
            }),
        }),
    }];
    let ctx = CoolContext::authenticated([(
        "email".to_owned(),
        Value::String("owner@example.com".to_owned()),
    )]);

    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index)
        .expect("policy preview should render");

    assert_eq!(
        sql,
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND email = $1)"
    );
}

#[test]
fn nested_relation_policy_preview_uses_recursive_exists_and_quantifiers() {
    let policy = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::Relation {
            quantifier: crate::RelationQuantifier::ToOne,
            parent_table: "posts",
            parent_column: "author_id",
            related_table: "users",
            related_column: "id",
            expr: &PolicyExpr::Predicate(ReadPredicate::Relation {
                quantifier: crate::RelationQuantifier::Every,
                parent_table: "users",
                parent_column: "id",
                related_table: "memberships",
                related_column: "user_id",
                expr: &PolicyExpr::Predicate(ReadPredicate::FieldEqLiteral {
                    column: "active",
                    value: crate::PolicyLiteral::Bool(true),
                }),
            }),
        }),
    }];
    let ctx = CoolContext::anonymous();

    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index)
        .expect("policy preview should render");

    assert_eq!(
        sql,
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND NOT EXISTS (SELECT 1 FROM memberships WHERE memberships.user_id = users.id AND NOT (active = $1)))"
    );
}

#[test]
fn filter_expr_and_or_flattens_matching_groups() {
    let left = FilterExpr::from(FieldRef::<(), i64>::new("id").eq(1_i64));
    let right = FilterExpr::from(FieldRef::<(), bool>::new("published").is_true());
    let third = FilterExpr::from(FieldRef::<(), String>::new("title").contains("Post"));

    let and_expr = left.clone().and(right.clone()).and(third.clone());
    let or_expr = left.or(right).or(third);

    assert!(matches!(and_expr, FilterExpr::All(filters) if filters.len() == 3));
    assert!(matches!(or_expr, FilterExpr::Any(filters) if filters.len() == 3));
}

#[test]
fn filter_expr_not_wraps_and_unwraps_double_negation() {
    let filter = FilterExpr::from(FieldRef::<(), bool>::new("published").is_true());
    let negated = filter.clone().not();
    let restored = negated.clone().not();
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&negated, &mut sql, &mut bind_index);

    assert_eq!(sql, "NOT (published = $1)");
    assert_eq!(restored, filter);
}

#[test]
fn update_preview_sql_unversioned_renders_simple_where_clause() {
    let sql = render_update_preview_sql(
        "accounts",
        "id",
        None,
        &["balance", "updated_at"],
        "id AS \"id\", balance AS \"balance\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, updated_at = $2 WHERE id = $3 RETURNING id AS \"id\", balance AS \"balance\""
    );
}

#[test]
fn update_preview_sql_versioned_bumps_version_and_filters_on_expected() {
    let sql = render_update_preview_sql(
        "accounts",
        "id",
        Some("version"),
        &["balance"],
        "id AS \"id\", version AS \"version\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, version = version + 1 WHERE id = $2 AND version = $3 RETURNING id AS \"id\", version AS \"version\""
    );
}

// ───── New builder verbs (#1 update_many preview SQL) ──────────────────────

use crate::query::render_update_many_preview_sql;

#[test]
fn update_many_preview_sql_unversioned_no_soft_delete() {
    let sql = render_update_many_preview_sql(
        "posts",
        false,
        None,
        &["title", "published"],
        "id AS \"id\", title AS \"title\"",
    );
    assert_eq!(
        sql,
        "UPDATE posts SET title = $1, published = $2 WHERE <filters> AND <update_policy> RETURNING id AS \"id\", title AS \"title\"",
    );
}

#[test]
fn update_many_preview_sql_versioned_bumps_version() {
    let sql = render_update_many_preview_sql(
        "accounts",
        false,
        Some("version"),
        &["balance"],
        "id AS \"id\", balance AS \"balance\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, version = version + 1 WHERE <filters> AND <update_policy> RETURNING id AS \"id\", balance AS \"balance\"",
    );
}

#[test]
fn update_many_preview_sql_with_soft_delete_layers_in_predicate() {
    let sql = render_update_many_preview_sql(
        "posts",
        true,
        None,
        &["title"],
        "id AS \"id\"",
    );
    assert_eq!(
        sql,
        "UPDATE posts SET title = $1 WHERE <soft_delete IS NULL> AND <filters> AND <update_policy> RETURNING id AS \"id\"",
    );
}
