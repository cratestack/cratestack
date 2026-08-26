//! The small, closed set of Postgres type-name aliases `pg_get_expr`'s
//! deparse can normalize an author-written cast into — Finding D
//! (cratestack#742, round 3 review): an author who writes `::int`
//! round-trips through introspection as `::integer`. Without this table,
//! both sides carry an explicit cast (so `super::super::segments_match`'s
//! one-side-lacking-a-cast tolerance doesn't apply), the type-name
//! strings differ, and the predicate compares unequal — a needless
//! drop+recreate on *every* `migrate` run, forever, for anyone who
//! writes an aliased spelling. A real churn hazard, the same ticket-
//! level failure Finding 1 (round 1) fixed for casts entirely, just for
//! a narrower population.
//!
//! Only ever called on a bare, unqualified, unquoted, undecorated name
//! (`super::parse_type_name`'s guards) — never on a schema-qualified,
//! double-quoted, or modifier-bearing spelling. `serial`/`bigserial`/
//! `smallserial` are deliberately absent: they aren't real column
//! *types* in this sense (they expand to an integer type plus a
//! sequence default at table-creation time — see
//! `emit::postgres::columns`), and a stored predicate's cast could never
//! legitimately name one, so there's nothing to alias.
///
/// An unrecognized name normalizes to itself — never guessed at, so a
/// mismatch on an unknown pair still fails toward churn (a needless
/// drop+recreate) rather than toward silent equality (a missed one),
/// matching this whole module's discipline.
pub(super) fn canonicalize(name: &str) -> &str {
    match name {
        "int" | "int4" => "integer",
        "int2" => "smallint",
        "int8" => "bigint",
        "float4" => "real",
        "float8" => "double precision",
        "varchar" => "character varying",
        "char" => "character",
        "bool" => "boolean",
        "decimal" => "numeric",
        "timestamptz" => "timestamp with time zone",
        "timetz" => "time with time zone",
        other => other,
    }
}
