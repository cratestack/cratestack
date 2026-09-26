use super::*;
use crate::RelatedReadScope;
use crate::filter::RelationQuantifier;

const fn to_one_hop(
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

static PROFILE_CATALOG: OrderCatalog = OrderCatalog {
    scalars: &[("nickname", "nickname")],
    relations: &[],
};

static USER_CATALOG: OrderCatalog = OrderCatalog {
    scalars: &[("email", "email")],
    relations: &[OrderRelationEdge {
        api_name: "profile",
        hop: to_one_hop("users", "profile_id", "profiles", "id"),
        target: &PROFILE_CATALOG,
    }],
};

static POST_CATALOG: OrderCatalog = OrderCatalog {
    scalars: &[("id", "id"), ("title", "title")],
    relations: &[OrderRelationEdge {
        api_name: "author",
        hop: to_one_hop("posts", "author_id", "users", "id"),
        target: &USER_CATALOG,
    }],
};

#[test]
fn resolves_own_scalar_with_no_hops() {
    let resolved = resolve_order_target(&POST_CATALOG, "title").expect("known scalar");
    assert!(resolved.hops.is_empty());
    assert_eq!(resolved.column, "title");
}

#[test]
fn resolves_single_hop_relation_scalar() {
    let resolved = resolve_order_target(&POST_CATALOG, "author.email").expect("known path");
    assert_eq!(
        resolved.hops,
        vec![to_one_hop("posts", "author_id", "users", "id")]
    );
    assert_eq!(resolved.column, "email");
}

#[test]
fn resolves_nested_two_hop_relation_scalar() {
    let resolved =
        resolve_order_target(&POST_CATALOG, "author.profile.nickname").expect("known path");
    assert_eq!(
        resolved.hops,
        vec![
            to_one_hop("posts", "author_id", "users", "id"),
            to_one_hop("users", "profile_id", "profiles", "id"),
        ]
    );
    assert_eq!(resolved.column, "nickname");
}

#[test]
fn resolved_hops_render_the_expected_nested_correlated_subquery() {
    // Closes the loop between "the resolver walked the right edges"
    // (above) and "those hops render the right SQL" -- through
    // `order_value_sql`, the unscoped renderer the embedded backend uses,
    // with the exact hops `resolve_order_target` produces for a two-hop
    // key. The Postgres backend renders the same hops with each hop's
    // read scope applied (`cratestack-sqlx`'s relation-scope tests).
    let resolved =
        resolve_order_target(&POST_CATALOG, "author.profile.nickname").expect("known path");
    assert_eq!(
        crate::order_value_sql(&resolved.hops, resolved.column),
        "(SELECT profiles.nickname FROM profiles \
         WHERE profiles.id = users.profile_id LIMIT 1)",
    );
}

#[test]
fn rejects_unknown_top_level_field() {
    assert!(resolve_order_target(&POST_CATALOG, "unknownField").is_none());
}

#[test]
fn rejects_unknown_relation_segment() {
    assert!(resolve_order_target(&POST_CATALOG, "editor.email").is_none());
}

#[test]
fn rejects_a_relation_named_key_with_no_terminal_scalar() {
    // "author" alone names a relation, not a scalar -- same
    // "unsupported sort field" outcome as any other unresolved key.
    assert!(resolve_order_target(&POST_CATALOG, "author").is_none());
}

#[test]
fn rejects_a_to_many_hop_because_it_is_never_in_the_catalog() {
    // The macro only ever emits to-one edges into `relations`, so a
    // key naming a to-many relation (e.g. "sessions") simply has no
    // matching edge -- exercised end to end in
    // `cratestack-pg/tests/include_schema.rs`'s
    // `axum_model_route_rejects_to_many_relation_order_by`.
    assert!(resolve_order_target(&USER_CATALOG, "sessions.label").is_none());
}
