//! Round 3 (cratestack#742, Finding D): once both sides of a predicate
//! carry an explicit `::type` cast, round 2's `segments_match` compared
//! the type names by exact string equality. Postgres normalizes an
//! alias on deparse — an author-written `::int` reads back as
//! `::integer` — so a schema whose author writes an aliased spelling
//! churned a drop+recreate on *every* `migrate` run, forever: the same
//! ticket-level "no churn" failure Finding 1 (round 1) fixed for casts
//! entirely, resurfacing for a narrower population. These pin the fix
//! (`predicate::casts::type_name::alias::canonicalize`) — a small,
//! closed alias table, applied only to a bare/unqualified/unquoted/
//! undecorated type name, that fails toward churn (never toward silent
//! equality) for anything it doesn't recognize.

use super::super::predicate::predicates_equivalent;

#[test]
fn int_and_integer_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::int", "x = '1'::integer"));
}

#[test]
fn int4_and_integer_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::int4", "x = '1'::integer"));
}

#[test]
fn varchar_and_character_varying_are_the_same_alias() {
    assert!(predicates_equivalent(
        "x = '1'::varchar",
        "x = '1'::character varying"
    ));
}

#[test]
fn float8_and_double_precision_are_the_same_alias() {
    assert!(predicates_equivalent(
        "x = '1'::float8",
        "x = '1'::double precision"
    ));
}

#[test]
fn int2_and_smallint_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::int2", "x = '1'::smallint"));
}

#[test]
fn int8_and_bigint_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::int8", "x = '1'::bigint"));
}

#[test]
fn float4_and_real_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::float4", "x = '1'::real"));
}

#[test]
fn char_and_character_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::char", "x = '1'::character"));
}

#[test]
fn bool_and_boolean_are_the_same_alias() {
    assert!(predicates_equivalent("x = '1'::bool", "x = '1'::boolean"));
}

#[test]
fn decimal_and_numeric_are_the_same_alias() {
    assert!(predicates_equivalent(
        "x = '1'::decimal",
        "x = '1'::numeric"
    ));
}

#[test]
fn timestamptz_and_timestamp_with_time_zone_are_the_same_alias() {
    assert!(predicates_equivalent(
        "x = '1'::timestamptz",
        "x = '1'::timestamp with time zone"
    ));
}

#[test]
fn timetz_and_time_with_time_zone_are_the_same_alias() {
    assert!(predicates_equivalent(
        "x = '1'::timetz",
        "x = '1'::time with time zone"
    ));
}

#[test]
fn alias_lookup_is_case_insensitive_for_unquoted_names() {
    // Unquoted SQL identifiers are case-insensitive — Postgres folds
    // them — so a differently-cased spelling of the same alias source
    // must still resolve to the same canonical type.
    assert!(predicates_equivalent("x = '1'::INT", "x = '1'::integer"));
}

#[test]
fn a_quoted_type_name_is_never_alias_normalized() {
    // `"int"` is a user-defined type literally named `int` — a real,
    // different type from the builtin `integer` alias, not a quoting
    // stylization of it. Must NOT match.
    assert!(!predicates_equivalent(
        "x = '1'::\"int\"",
        "x = '1'::integer"
    ));
}

#[test]
fn schema_qualified_alias_source_is_not_normalized() {
    // A schema-qualified spelling isn't a bare alias source in the
    // first place (`parse_type_name`'s `decorated` guard) — stays an
    // exact-match comparison, consistent with
    // `schema_qualified_and_bare_type_names_do_not_match` in
    // `type_safety.rs`.
    assert!(!predicates_equivalent(
        "x = '1'::pg_catalog.int",
        "x = '1'::integer"
    ));
}

#[test]
fn an_unrecognized_type_name_pair_still_does_not_match() {
    // The guardrail: an unknown name normalizes to itself, never to
    // "assume equal". Two different unknown types must still churn.
    assert!(!predicates_equivalent(
        "x = '1'::mytype",
        "x = '1'::othertype"
    ));
}

#[test]
fn citext_and_text_are_never_aliased_together() {
    // `citext` is not in the alias table and never should be — it's
    // case-insensitive, `text` is not, and conflating them would
    // reopen Finding A's exact money-relevant failure. Restated here
    // (also pinned in `type_safety.rs`) because it's the one alias
    // addition this fix must never accidentally make.
    assert!(!predicates_equivalent(
        "email = 'x'::citext",
        "email = 'x'::text"
    ));
}

#[test]
fn a_decorated_alias_source_is_not_normalized() {
    // A modifier attached to an alias source (`decimal(10,2)`) isn't a
    // bare alias source either — stays exact-match, so it doesn't
    // compare equal to the differently-decorated canonical spelling.
    // A conservative (churn-favoring) gap, not a correctness bug: see
    // `type_name`'s module doc.
    assert!(!predicates_equivalent(
        "x = '1'::decimal(10,2)",
        "x = '1'::numeric(10,2)"
    ));
}

/// Round 4 (cratestack#742, coordinator review): pins a real, known
/// limitation the alias fix does NOT close, so it's executable rather
/// than living only in a report. Empirically verified (a throwaway
/// container, `psql`, cratestack#742's verification discipline) —
/// `CREATE INDEX ... WHERE email = 'x'::varchar` against a `text`
/// column deparses via `pg_get_expr` as
/// `(email = ('x'::character varying)::text)`: comparing a
/// `character varying`-cast literal to a `text` column needs an
/// *additional*, implicit cast back to `text`, which Postgres nests
/// around the whole expression rather than folding away. That's a
/// second `::text` cast the schema's own literal `email = 'x'::varchar`
/// never had, so the segment sequences have different shapes — one more
/// `Other`/`Literal` boundary than the schema side — and the mismatch is
/// structural, not a type-name spelling disagreement the alias table
/// (`type_name::alias`) could ever resolve, however complete. This is
/// why `partial_index_with_aliased_cast_type_round_trips_without_churn`
/// (`tests/postgres_introspect.rs`) proves the alias fix using
/// `int8`/`bigint` — a genuinely clean single-cast round-trip — instead
/// of `varchar`. An author who writes `::varchar` still gets a real
/// migration (churn), not a corruption or a silent wrong result: this
/// is the same *safe* failure direction as every other undecorated
/// shape this module declines to handle, just pinned explicitly instead
/// of left to be rediscovered.
#[test]
fn varchar_on_a_text_column_still_churns_due_to_the_extra_implicit_cast() {
    assert!(!predicates_equivalent(
        "email = 'x'::varchar",
        "(email = ('x'::character varying)::text)"
    ));
}
