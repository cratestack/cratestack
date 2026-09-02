//! `query` block parse + semantic-check coverage (cratestack#867;
//! accepted design `docs/design/declarative-custom-query.md`).
//!
//! The acceptance criteria these back are the parse-time half of the
//! ticket: a `query` parses into IR with its args, result type and SQL
//! body intact, and every way of getting it wrong is rejected *here*
//! rather than at runtime. The "declared result type vs. real `SELECT`
//! list" correspondence is deliberately absent — design §3 prices the
//! inference that would be needed and rejects it; that mismatch surfaces
//! as a loud `sqlx` decode error at first execution, exactly as it
//! already does for `view`.

use crate::parse_schema;

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

fn error_for(source: &str) -> String {
    parse_schema(source)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_else(|| panic!("expected schema to be rejected, but it parsed"))
}

fn with_query(declaration: &str) -> String {
    format!(
        r#"
type Totals {{
  total Int
}}

{declaration}
"#
    )
}

#[test]
fn rejects_a_placeholder_past_the_declared_parameter_count() {
    let message = error_for(&with_query(
        r#"query totals(userId: String, cutoff: DateTime): Totals
  @@sql("SELECT 1 AS total WHERE a = $1 AND b = $3")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("references parameter `$3`"), "{message}");
    assert!(
        message.contains("only 2 parameter(s) are declared (`userId`, `cutoff`)"),
        "{message}"
    );
}

#[test]
fn rejects_a_zero_placeholder() {
    // Postgres parameters are 1-based; `$0` would fail at bind time with
    // an error pointing inside generated code.
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $0")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("references parameter `$0`"), "{message}");
}

#[test]
fn rejects_a_declared_parameter_that_the_body_never_references() {
    // The typo the epic worried about: `$3` written for `$2` leaves
    // `cutoff` silently unused. Checking only the other direction would
    // catch the `$3` but not tell the author which parameter went dead.
    let message = error_for(&with_query(
        r#"query totals(userId: String, cutoff: DateTime): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("declares parameter `cutoff` (`$2`) but it is never referenced"),
        "{message}"
    );
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
fn rejects_an_unknown_result_type() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Nope
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("unknown result type"), "{message}");
    assert!(message.contains("no `type Nope` is declared"), "{message}");
}

#[test]
fn rejects_a_model_as_the_result_type() {
    // Design §6: a `query`'s raw SQL gets no soft-delete or row-policy
    // filtering, so handing back a `Model` would look like a filtered
    // model read when it is nothing of the kind.
    let message = error_for(
        r#"
model Event {
  id Int @id
}

query totals(userId: String): Event
  @@sql("SELECT 1 AS id WHERE a = $1")
  @allow(auth() != null)
"#,
    );
    assert!(
        message.contains("is not a `type` declaration"),
        "{message}"
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

#[test]
fn rejects_a_query_with_no_sql_body() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("has no SQL body"), "{message}");
}

#[test]
fn rejects_the_per_backend_sql_split_a_view_allows() {
    // `query` is Postgres-only (design §4). Accepting `@@embedded_sql`'s
    // spelling would advertise a backend that does not exist.
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@embedded_sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("Postgres-only"), "{message}");
    assert!(message.contains("@@embedded_sql"), "{message}");
}

#[test]
fn rejects_an_unsupported_attribute() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)
  @stream"#,
    ));
    assert!(
        message.contains("unsupported attribute `@stream`"),
        "{message}"
    );
}

#[test]
fn rejects_an_unbindable_parameter_type() {
    let message = error_for(&with_query(
        r#"query totals(page: PageInput): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("cannot be bound as a SQL parameter"),
        "{message}"
    );
}

#[test]
fn rejects_a_list_parameter() {
    let message = error_for(&with_query(
        r#"query totals(ids: String[]): Totals
  @@sql("SELECT 1 AS total WHERE a = ANY($1)")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("must be a required scalar"), "{message}");
}

#[test]
fn rejects_a_duplicate_query_name() {
    let message = error_for(&with_query(
        r#"query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)

query totals(userId: String): Totals
  @@sql("SELECT 2 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(message.contains("duplicate query name `totals`"), "{message}");
}

#[test]
fn rejects_two_queries_that_would_generate_the_same_module() {
    let message = error_for(&with_query(
        r#"query monthTotals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)

query month_totals(userId: String): Totals
  @@sql("SELECT 2 AS total WHERE a = $1")
  @allow(auth() != null)"#,
    ));
    assert!(
        message.contains("both generate the module `month_totals`"),
        "{message}"
    );
}

#[test]
fn rejects_a_query_when_the_schema_configures_no_database() {
    let message = error_for(
        r#"
datasource db {
  provider = "none"
}

type Totals {
  total Int
}

query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)
"#,
    );
    assert!(
        message.contains("configures no database for a `query` to run against"),
        "{message}"
    );
}

#[test]
fn rejects_a_header_with_no_result_type() {
    let message = error_for(&with_query(
        r#"query totals(userId: String)
  @@sql("SELECT 1 AS total WHERE a = $1")"#,
    ));
    assert!(message.contains("must include a result type"), "{message}");
}
