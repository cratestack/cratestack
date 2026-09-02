//! What a `query` block parses *into* (cratestack#867; accepted design
//! `docs/design/declarative-custom-query.md`).
//!
//! The rejection half lives in the sibling
//! [`tests_queries_rejections`](crate::tests_queries_rejections), split
//! for the workspace's 200-line file ceiling; read the two together.
//!
//! One thing deliberately absent from both: any check that the declared
//! result type matches the SQL's real `SELECT` list. Design §3 prices the
//! inference that would need — a genuine SQL expression type-checker,
//! spanning casts, aggregates and cross-model column lookups — and
//! rejects it. That mismatch surfaces as a loud `sqlx` decode error at
//! first execution, exactly as it already does for `view`.

use crate::parse_schema;
use crate::tests_queries_support::with_query;

const MOTIVATING: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  subjectId String
}

type LoyaltyFeeSummary {
  total Int
  thisMonth Int
}

query loyaltyFeeSummary(userId: String, cutoff: DateTime): LoyaltyFeeSummary
  @@sql("""
    SELECT
      COALESCE(SUM(discount), 0)::bigint AS "total",
      COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS "thisMonth"
    FROM loyalty_fee_events
    WHERE user_id = $1
  """)
  @allow(auth() != null)
"#;

#[test]
fn parses_the_designs_motivating_query() {
    let schema = parse_schema(MOTIVATING).expect("schema should parse");

    assert_eq!(schema.queries.len(), 1);
    let query = &schema.queries[0];
    assert_eq!(query.name, "loyaltyFeeSummary");
    assert_eq!(query.result_type.name, "LoyaltyFeeSummary");
    assert_eq!(
        query
            .args
            .iter()
            .map(|arg| (arg.name.as_str(), arg.ty.name.as_str()))
            .collect::<Vec<_>>(),
        vec![("userId", "String"), ("cutoff", "DateTime")],
    );

    // The body survives verbatim, newlines and all — the generated
    // `const SQL` is this string, never a rewritten one.
    let sql = query.sql().expect("query should carry a SQL body");
    assert!(sql.contains("FILTER (WHERE created_at >= $2)"), "{sql}");
    assert!(sql.contains("WHERE user_id = $1"), "{sql}");
    assert!(sql.contains('\n'), "multi-line body should keep newlines");
}

#[test]
fn a_query_does_not_become_a_procedure() {
    // The whole no-client-surface guarantee rests on `query` never
    // landing in `schema.procedures`, which every route/op-descriptor/
    // client-stub emission site iterates (design §1/§5).
    let schema = parse_schema(MOTIVATING).expect("schema should parse");
    assert!(schema.procedures.is_empty());
    assert!(schema.views.is_empty());
}

#[test]
fn accepts_a_parameterless_query() {
    // Design §8's self-test: the range check is vacuous at `1..=0`, and
    // no special case should be needed for it.
    let schema = parse_schema(&with_query(
        r#"query totals(): Totals
  @@sql("SELECT COUNT(*)::bigint AS total FROM events")
  @allow(auth() != null)"#,
    ))
    .expect("a zero-parameter query should parse");
    assert!(schema.queries[0].args.is_empty());
}

#[test]
fn accepts_a_list_result_type() {
    let schema = parse_schema(&with_query(
        r#"query totals(userId: String): Totals[]
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ))
    .expect("a list-returning query should parse");
    assert_eq!(
        schema.queries[0].result_type.arity,
        cratestack_core::TypeArity::List
    );
}

#[test]
fn an_empty_policy_parses_and_is_left_to_deny_at_runtime() {
    // Deny-by-default is `authorize_procedure`'s empty-allow-list rule,
    // not a parse error — a `query` with no `@allow` is a legal schema
    // that nobody can call. Asserting the *parse* half here; the runtime
    // half is asserted against real Postgres in `cratestack-pg`.
    let schema = parse_schema(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")"#,
    ))
    .expect("a policy-less query should parse");
    assert!(
        !schema.queries[0]
            .attributes
            .iter()
            .any(|attribute| attribute.raw.starts_with("@allow"))
    );
}
