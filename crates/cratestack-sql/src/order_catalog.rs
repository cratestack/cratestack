//! Runtime dotted-key resolution for REST `?orderBy=`/`?sort=` keys that
//! cross to-one relations (`"author.profile.nickname"`).
//!
//! Mirrors the type-space-to-value-space move `relation_path` already made
//! for the typed builder (cratestack#253) — one `OrderCatalog` per model,
//! carrying only that model's own scalar columns and its own to-one
//! relation edges, rather than a pre-enumerated list of every dotted path
//! through the graph. [`resolve_order_target`] walks a key hop by hop
//! against these catalogs at request time, so codegen for the REST
//! dispatch surface stays linear in `models × fields` however densely
//! models are to-one-connected (cratestack#256 — the same exponential
//! shape as #252, but in the REST string-key match arms rather than the
//! typed builder's path types).

use crate::relation_path::RelationHop;

/// One model's order-by surface: its own sortable scalar columns
/// (`(api_name, sql_column)`) and its own to-one relation edges. Exactly
/// one `OrderCatalog` is emitted per model, regardless of how many
/// distinct relation paths pass through it.
pub struct OrderCatalog {
    pub scalars: &'static [(&'static str, &'static str)],
    pub relations: &'static [OrderRelationEdge],
}

/// One to-one relation edge out of a model. `target` points at the
/// related model's own catalog so [`resolve_order_target`] can keep
/// walking further segments; to-many relations are never represented
/// here (mirroring the codegen's existing to-one-only walk), so a key
/// that names one simply fails to resolve.
pub struct OrderRelationEdge {
    pub api_name: &'static str,
    pub hop: RelationHop,
    pub target: &'static OrderCatalog,
}

/// A dotted sort key resolved down to the relation hops to traverse plus
/// the terminal scalar column, ready for [`crate::order_value_sql`].
pub struct ResolvedOrderTarget {
    pub hops: Vec<RelationHop>,
    pub column: &'static str,
}

/// Walk `key` (dot-separated, e.g. `"author.profile.nickname"`) through
/// `catalog`, following to-one relation edges one segment at a time and
/// resolving the final segment against the current model's scalar
/// columns.
///
/// Returns `None` for an unknown field, a relation segment with no
/// matching edge (including any to-many hop, which is never present in
/// the catalog), or a key whose last segment names a relation instead of
/// a scalar — every one of which the caller reports as the same
/// "unsupported sort field" validation error as any other bad key.
pub fn resolve_order_target(
    catalog: &'static OrderCatalog,
    key: &str,
) -> Option<ResolvedOrderTarget> {
    let mut hops = Vec::new();
    let mut current = catalog;
    let mut segments = key.split('.').peekable();

    loop {
        let segment = segments.next()?;
        if segments.peek().is_none() {
            return current
                .scalars
                .iter()
                .find(|(name, _)| *name == segment)
                .map(|(_, column)| ResolvedOrderTarget { hops, column });
        }
        let edge = current
            .relations
            .iter()
            .find(|edge| edge.api_name == segment)?;
        hops.push(edge.hop);
        current = edge.target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // (above) and "those hops render the right SQL" -- the REST
        // dispatcher's only other consumer of `resolved.hops` is
        // `order_value_sql`, exercised here with the exact same output
        // `resolve_order_target` produces for a two-hop key. Mirrors the
        // typed-builder assertion in
        // `cratestack-pg/tests/include_schema.rs`'s
        // `generated_nested_relation_order_preview_renders_nested_subqueries`,
        // which hits the identical `order_value_sql` primitive through the
        // chained-accessor path instead of the REST dotted-key path.
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
}
