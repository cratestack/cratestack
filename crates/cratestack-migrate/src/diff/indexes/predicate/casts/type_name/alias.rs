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
//!
//! **Scope, precisely (round 4 review):** this table only settles
//! whether two `::type` spellings NAME the same type — it says nothing
//! about, and cannot fix, a *structural* difference in how Postgres
//! wraps a cast. Some aliased spellings compared against a column of a
//! different-but-implicitly-compatible type deparse with an *extra*,
//! nested implicit cast the schema's own literal text never had.
//! Empirically verified (a throwaway container, `psql`, this ticket's
//! verification discipline): `WHERE email = 'x'::varchar` against a
//! `text` column deparses as
//! `(email = ('x'::character varying)::text)` — the `varchar`→
//! `character varying` alias resolves correctly, but the surrounding
//! `('x'::character varying)::text` wrapper is a second cast the
//! schema's `'x'::varchar` never had, so the two sides' segment
//! sequences differ in *shape*, not just in one literal's type name,
//! and still compare unequal (churn). This table cannot and does not
//! address that — no alias table can, since it isn't an aliasing
//! problem — so an author who writes `::varchar` against a `text`
//! column still gets a real migration on every `pg_get_expr` round-trip,
//! not the clean no-op `int8`/`bigint` (this table's actually-proven
//! case, see `tests/postgres_introspect.rs`'s
//! `partial_index_with_aliased_cast_type_round_trips_without_churn`)
//! gets. That's the *safe* direction (churn, not corruption or a missed
//! constraint), so it's a known, pinned limitation
//! (`tests::alias::varchar_on_a_text_column_still_churns_due_to_the_extra_implicit_cast`),
//! not a regression.
//!
//! An unrecognized name normalizes to itself — never guessed at, so a
//! mismatch on an unknown pair still fails toward churn (a needless
//! drop+recreate) rather than toward silent equality (a missed one),
//! matching this whole module's discipline.
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
