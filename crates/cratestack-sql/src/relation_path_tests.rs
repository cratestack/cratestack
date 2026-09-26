use super::*;
use crate::FieldRef;
use cratestack_policy::{PolicyExpr, ReadPolicy, ReadPredicate};

static USERS_ALLOW: [ReadPolicy; 1] = [ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
}];

const USERS_SCOPE: RelatedReadScope = RelatedReadScope::Policy {
    allow: &USERS_ALLOW,
    deny: &[],
    soft_delete_column: Some("deleted_at"),
};

const fn to_one(
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
) -> RelationHop {
    RelationHop::new(
        parent_table,
        parent_column,
        related_table,
        related_column,
        RelationQuantifier::ToOne,
        RelatedReadScope::Unscoped,
    )
}

#[test]
fn single_hop_reads_the_related_table_directly() {
    let hops = [to_one("posts", "author_id", "users", "id")];
    assert_eq!(order_value_sql(&hops, "email"), "users.email");
}

#[test]
fn two_hops_nest_a_correlated_subquery() {
    let hops = [
        to_one("posts", "author_id", "users", "id"),
        to_one("users", "profile_id", "profiles", "id"),
    ];
    assert_eq!(
        order_value_sql(&hops, "nickname"),
        "(SELECT profiles.nickname FROM profiles \
         WHERE profiles.id = users.profile_id LIMIT 1)",
    );
}

#[test]
fn a_to_many_hop_makes_the_path_unorderable() {
    let hops = [
        to_one("posts", "author_id", "users", "id"),
        to_one("users", "id", "comments", "user_id").with_quantifier(RelationQuantifier::Some),
    ];
    assert!(!is_orderable(&hops));
    assert!(is_orderable(&hops[..1]));
}

/// Every relation node `wrap_filter` builds must carry its own hop's
/// scope — not the first hop's, not the last hop's, and never a default.
#[test]
fn wrap_filter_carries_each_hops_scope_onto_its_relation_node() {
    let hops = [
        RelationHop::new(
            "posts",
            "author_id",
            "users",
            "id",
            RelationQuantifier::ToOne,
            USERS_SCOPE,
        ),
        to_one("users", "id", "comments", "user_id").with_quantifier(RelationQuantifier::Every),
    ];
    let leaf: FilterExpr = FieldRef::<(), String>::new("body")
        .eq("x".to_owned())
        .into();
    let FilterExpr::Relation(outer) = wrap_filter(&hops, leaf) else {
        panic!("outer node must be a relation");
    };
    assert_eq!(outer.quantifier, RelationQuantifier::ToOne);
    assert_eq!(outer.related_table, "users");
    assert_eq!(outer.scope, USERS_SCOPE);
    let FilterExpr::Relation(inner) = *outer.filter else {
        panic!("inner node must be a relation");
    };
    assert_eq!(inner.quantifier, RelationQuantifier::Every);
    assert_eq!(inner.related_table, "comments");
    assert_eq!(inner.scope, RelatedReadScope::Unscoped);
}

#[test]
fn only_a_hop_back_into_its_own_table_is_a_self_relation() {
    assert!(to_one("users", "manager_id", "users", "id").is_self_relation());
    assert!(!to_one("posts", "author_id", "users", "id").is_self_relation());
}
