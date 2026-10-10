//! ADR 0019 PR B (B11): policies over `BigInt` fields and arguments must deny
//! what they say they deny. Real Postgres.
//!
//! The risk (ADR 0019 risk 2, plan section 4): a NEGATED predicate on a
//! `BigInt` fails OPEN. A create policy compares the input's `BigInt` value with
//! a literal or auth claim that was built as `Int`; a procedure policy reads a
//! `BigInt` argument through a `Value` conversion. Where either has no `BigInt`
//! arm the comparison is "no match", so `!=` and `not in` evaluate true and the
//! operation is ALLOWED when the values are EQUAL. Nothing fails to compile and
//! nothing logs; a green `==` test would not notice. So every shape is exercised
//! in both directions (the equal value must deny, a different one must allow),
//! at `i64::MAX`, `i64::MIN` and `2^53 + 1`, which also catches a comparison
//! made through a narrower type.
//!
//! Failures are collected and reported together, so one run shows every shape
//! that is broken, not only the first.

use cratestack::axum::Router;
use cratestack::axum::http::{HeaderMap, StatusCode};
use cratestack::serde_json::{Value as Json, json};
use cratestack::sqlx::PgPool;
use cratestack::{
    BigInt, CodecSet, CratestackContext, CratestackError, Value, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

include_server_schema!("tests/fixtures/bigint_policies.cstack", db = Postgres);

mod bigint_support;
mod support;

use bigint_support::{ABOVE_2_53, I64_MAX, I64_MIN, Wire, call, column_type, reset_with};
use cratestack_schema::procedures::{bi_direct_ne, bi_direct_ne_auth, bi_isolated_ne};
use cratestack_schema::{BiPair, CreateBiPolStampedInput, IsolatedCratestack};
use support::pg;

const SCHEMA: &str = include_str!("fixtures/bigint_policies.cstack");
const TABLES: &str = "bi_pol_ne_lits, bi_pol_not_ins, bi_pol_ne_auths, bi_pol_deny_eqs, \
     bi_pol_deny_ins, bi_pol_eq_lits, bi_pol_eq_auths, bi_pol_ins, bi_pol_scopeds, \
     bi_pol_ne_reads, bi_pol_stampeds";

const A53P1: i64 = 9_007_199_254_740_993;
const A53: i64 = 9_007_199_254_740_992;
const A53P2: i64 = 9_007_199_254_740_994;

fn caller(quota: Option<Value>) -> CratestackContext {
    let mut claims = vec![("id".to_owned(), Value::Int(1))];
    if let Some(quota) = quota {
        claims.push(("quotaE8".to_owned(), quota));
    }
    CratestackContext::authenticated(claims)
}

/// Applies the emitted DDL for the fixture. The emitter copies the
/// auth-derived `@default(auth().quotaE8)` of `BiPolStamped` into the column as
/// `DEFAULT auth().quotaE8`, which Postgres cannot run; the runtime fills that
/// column, so the clause is dropped. This is true of any scalar, not only BigInt.
async fn reset(pool: &PgPool) {
    reset_with(pool, SCHEMA, TABLES, |up| {
        up.replace(" DEFAULT auth().quotaE8", "")
    })
    .await;
}

/// Auth for the HTTP tests: the caller's `quotaE8` claim comes from a header,
/// as an integer (`x-quota-int`) or as a decimal string (`x-quota-str`).
fn header_auth(headers: &HeaderMap) -> Result<CratestackContext, CratestackError> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let quota = match (text("x-quota-int"), text("x-quota-str")) {
        (Some(n), _) => Some(Value::Int(n.parse().expect("x-quota-int is an i64"))),
        (None, Some(s)) => Some(Value::String(s.to_owned())),
        (None, None) => None,
    };
    Ok(caller(quota))
}

// ---------------------------------------------------------------------------
// model create policies (evaluated in Rust against the input)
// ---------------------------------------------------------------------------

macro_rules! try_create {
    ($failures:ident, $next_id:ident, $cool:expr, $ctx:expr, $model:literal,
     $accessor:ident, $input:ident, $amount:expr, $allowed:expr) => {{
        $next_id += 1;
        let amount: i64 = $amount;
        let allowed: bool = $allowed;
        let result = $cool
            .$accessor()
            .create(cratestack_schema::$input {
                id: $next_id,
                amountE8: BigInt::new(amount),
            })
            .run($ctx)
            .await;
        match (&result, allowed) {
            (Ok(_), true) => {}
            (Err(error), false) if error.code() == "FORBIDDEN" => {}
            (Ok(_), false) => $failures.push(format!(
                "{}: amount {amount} was ALLOWED; the policy must deny it (fails open)",
                $model
            )),
            (Err(error), _) => $failures.push(format!(
                "{}: amount {amount}: expected {}, got {} {:?}",
                $model,
                if allowed { "allow" } else { "FORBIDDEN" },
                error.code(),
                error.detail()
            )),
        }
    }};
}

#[tokio::test]
async fn create_policies_with_ne_not_in_and_eq_on_a_bigint_column_deny_what_they_say() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let cool = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let ctx = caller(Some(Value::Int(A53P1)));
    let mut failures: Vec<String> = Vec::new();
    let mut id = 0_i64;

    // `amountE8 != 100`
    for (amount, allowed) in [
        (100, false),
        (101, true),
        (0, true),
        (A53P1, true),
        (i64::MAX, true),
        (i64::MIN, true),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolNeLit (!= 100)",
            bi_pol_ne_lit,
            CreateBiPolNeLitInput,
            amount,
            allowed
        );
    }

    // `amountE8 not in [100, 2^53+1, i64::MAX, i64::MIN]`
    for (amount, allowed) in [
        (100, false),
        (A53P1, false),
        (i64::MAX, false),
        (i64::MIN, false),
        (101, true),
        (A53, true),
        (A53P2, true),
        (0, true),
        (i64::MIN + 1, true),
        (i64::MAX - 1, true),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolNotIn (not in)",
            bi_pol_not_in,
            CreateBiPolNotInInput,
            amount,
            allowed
        );
    }

    // `amountE8 != auth().quotaE8`, the claim being 2^53+1
    for (amount, allowed) in [
        (A53P1, false),
        (A53, true),
        (A53P2, true),
        (i64::MAX, true),
        (100, true),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolNeAuth (!= auth), claim 2^53+1",
            bi_pol_ne_auth,
            CreateBiPolNeAuthInput,
            amount,
            allowed
        );
    }
    for (claim, amount, allowed) in [
        (i64::MAX, i64::MAX, false),
        (i64::MAX, A53P1, true),
        (i64::MIN, i64::MIN, false),
        (i64::MIN, i64::MAX, true),
    ] {
        let ctx = caller(Some(Value::Int(claim)));
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolNeAuth (!= auth), claim at the extremes",
            bi_pol_ne_auth,
            CreateBiPolNeAuthInput,
            amount,
            allowed
        );
    }

    // `@@allow(create, auth() != null)` + `@@deny(create, amountE8 == 100)`
    for (amount, allowed) in [(100, false), (101, true), (A53P1, true), (i64::MAX, true)] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolDenyEq (@@deny ==)",
            bi_pol_deny_eq,
            CreateBiPolDenyEqInput,
            amount,
            allowed
        );
    }

    // `@@deny(create, amountE8 in [100, 2^53+1])`
    for (amount, allowed) in [
        (100, false),
        (A53P1, false),
        (101, true),
        (A53, true),
        (i64::MAX, true),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolDenyIn (@@deny in)",
            bi_pol_deny_in,
            CreateBiPolDenyInInput,
            amount,
            allowed
        );
    }

    // `@@allow(create, amountE8 == 100)`
    for (amount, allowed) in [
        (100, true),
        (101, false),
        (A53P1, false),
        (i64::MAX, false),
        (i64::MIN, false),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolEqLit (== 100)",
            bi_pol_eq_lit,
            CreateBiPolEqLitInput,
            amount,
            allowed
        );
    }

    // `@@allow(create, amountE8 == auth().quotaE8)`, the claim being 2^53+1
    for (amount, allowed) in [
        (A53P1, true),
        (A53, false),
        (A53P2, false),
        (i64::MAX, false),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolEqAuth (== auth), claim 2^53+1",
            bi_pol_eq_auth,
            CreateBiPolEqAuthInput,
            amount,
            allowed
        );
    }

    // `@@allow(create, amountE8 in [100, i64::MAX])`
    for (amount, allowed) in [
        (100, true),
        (i64::MAX, true),
        (101, false),
        (i64::MIN, false),
        (A53P1, false),
    ] {
        try_create!(
            failures,
            id,
            cool,
            &ctx,
            "BiPolIn (in)",
            bi_pol_in,
            CreateBiPolInInput,
            amount,
            allowed
        );
    }

    assert!(
        failures.is_empty(),
        "{} policy case(s) wrong:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn a_rest_create_with_a_not_equal_policy_is_a_403_when_the_values_are_equal() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let router = router(&test_pg.pool);
    for wire in Wire::BOTH {
        let mut next = if wire == Wire::Json { 0 } else { 100 };
        for (amount, status) in [
            ("100", StatusCode::FORBIDDEN),
            ("101", StatusCode::CREATED),
            (ABOVE_2_53, StatusCode::CREATED),
            (I64_MAX, StatusCode::CREATED),
            (I64_MIN, StatusCode::CREATED),
        ] {
            next += 1;
            let reply = call(
                &router,
                "POST",
                "/bi_pol_ne_lits",
                wire,
                &[],
                Some(&json!({"id": next, "amountE8": amount})),
            )
            .await;
            assert_eq!(
                reply.status,
                status,
                "{wire:?} amount {amount}: {}",
                reply.json()
            );
        }
        // The extreme literal in a `not in` list.
        for (amount, status) in [
            (I64_MAX, StatusCode::FORBIDDEN),
            (I64_MIN, StatusCode::FORBIDDEN),
            (ABOVE_2_53, StatusCode::FORBIDDEN),
            ("9007199254740994", StatusCode::CREATED),
        ] {
            next += 1;
            let reply = call(
                &router,
                "POST",
                "/bi_pol_not_ins",
                wire,
                &[],
                Some(&json!({"id": next, "amountE8": amount})),
            )
            .await;
            assert_eq!(reply.status, status, "{wire:?} not in: amount {amount}");
        }
    }
}

// ---------------------------------------------------------------------------
// the same shapes, with the claim delivered as a decimal string
// ---------------------------------------------------------------------------

/// A `BigInt` claim that arrives as its canonical decimal string (a JWT claim
/// above 2^53 has to) cannot be compared with a `BigInt` column: the predicate
/// carries no column type, so the string is undecidable and DENIES for `==` and
/// for `!=`, equal or not. Allowing `!=` when the numbers are equal is the
/// fail-open (ADR 0019 risk 2, risk 8). An integer claim is the way to compare.
#[tokio::test]
async fn a_string_claim_in_a_create_policy_comparison_is_undecidable_and_denies_both_ways() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let cool = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let mut failures: Vec<String> = Vec::new();
    let mut id = 0_i64;
    for claim in [ABOVE_2_53, "5", "9223372036854775807"] {
        let ctx = caller(Some(Value::String(claim.to_owned())));
        // Equal to the claim, and not.
        for amount in [claim.parse::<i64>().expect("fits"), 7] {
            try_create!(
                failures,
                id,
                cool,
                &ctx,
                "BiPolNeAuth (!= auth), string claim",
                bi_pol_ne_auth,
                CreateBiPolNeAuthInput,
                amount,
                false
            );
            try_create!(
                failures,
                id,
                cool,
                &ctx,
                "BiPolEqAuth (== auth), string claim",
                bi_pol_eq_auth,
                CreateBiPolEqAuthInput,
                amount,
                false
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} case(s) wrong:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let rows: i64 = cratestack::sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM bi_pol_ne_auths) + (SELECT count(*) FROM bi_pol_eq_auths)",
    )
    .fetch_one(&test_pg.pool)
    .await
    .expect("count");
    assert_eq!(rows, 0, "a denied create must write nothing");
}

// ---------------------------------------------------------------------------
// read policies (SQL)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_policies_on_a_bigint_column_filter_rows_exactly() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    for (id, owner, amount) in [
        (1_i64, A53P1, 5_i64),
        (2, A53P1, 100),
        (3, i64::MAX, 5),
        (4, A53P1, i64::MAX),
        (5, i64::MIN, 7),
    ] {
        cratestack::sqlx::query(
            "INSERT INTO bi_pol_scopeds (id, owner_e8, amount_e8) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(owner)
        .bind(amount)
        .execute(pool)
        .await
        .expect("seed scoped");
    }
    for (id, amount) in [(1_i64, A53P1), (2, A53), (3, i64::MAX), (4, i64::MIN)] {
        cratestack::sqlx::query("INSERT INTO bi_pol_ne_reads (id, amount_e8) VALUES ($1, $2)")
            .bind(id)
            .bind(amount)
            .execute(pool)
            .await
            .expect("seed ne reads");
    }
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // `@@allow(read, ownerE8 == auth().quotaE8)`, `@@deny(read, amountE8 == 100)`
    for (quota, expected) in [
        (A53P1, vec![1_i64, 4]),
        (i64::MAX, vec![3]),
        (i64::MIN, vec![5]),
        (42, vec![]),
    ] {
        let rows = cool
            .bi_pol_scoped()
            .find_many()
            .order_by(cratestack_schema::bi_pol_scoped::id().asc())
            .run(&caller(Some(Value::Int(quota))))
            .await
            .expect("scoped read");
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            expected,
            "claim {quota}"
        );
    }

    // `@@allow(read, amountE8 != auth().quotaE8)`: the claim's row is hidden.
    for (quota, expected) in [
        (A53P1, vec![2_i64, 3, 4]),
        (i64::MAX, vec![1, 2, 4]),
        (i64::MIN, vec![1, 2, 3]),
    ] {
        let rows = cool
            .bi_pol_ne_read()
            .find_many()
            .order_by(cratestack_schema::bi_pol_ne_read::id().asc())
            .run(&caller(Some(Value::Int(quota))))
            .await
            .expect("ne read");
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            expected,
            "!= claim {quota}"
        );
    }

    // A string claim binds as TEXT against a BIGINT column. Postgres has no such
    // operator (SQLSTATE 42883), so the read fails instead of matching, and
    // `!=` cannot open the table.
    for (label, result) in [
        (
            "==",
            cool.bi_pol_scoped()
                .find_many()
                .run(&caller(Some(Value::String(A53P1.to_string()))))
                .await
                .map(|rows| rows.len()),
        ),
        (
            "!=",
            cool.bi_pol_ne_read()
                .find_many()
                .run(&caller(Some(Value::String(A53P1.to_string()))))
                .await
                .map(|rows| rows.len()),
        ),
    ] {
        let error = result.expect_err(&format!(
            "a string claim in a `{label}` read policy must not return rows"
        ));
        assert_eq!(error.db_sqlstate(), Some("42883"), "`{label}`: {error:?}");
    }
}

// ---------------------------------------------------------------------------
// an auth-derived default fills a BigInt column
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_auth_derived_default_fills_a_bigint_column_from_an_int_or_a_canonical_string() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    assert_eq!(
        column_type(pool, "bi_pol_stampeds", "owner_e8").await,
        "bigint"
    );
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let create = |id: i64, ctx: CratestackContext| {
        let cool = cool.clone();
        async move {
            cool.bi_pol_stamped()
                .create(CreateBiPolStampedInput {
                    id,
                    note: "n".to_owned(),
                })
                .run(&ctx)
                .await
        }
    };

    for (id, claim, expected) in [
        (1_i64, Value::Int(i64::MAX), i64::MAX),
        (2, Value::Int(i64::MIN), i64::MIN),
        (3, Value::Int(A53P1), A53P1),
        (4, Value::String(i64::MAX.to_string()), i64::MAX),
        (5, Value::String(A53P1.to_string()), A53P1),
        (6, Value::String(i64::MIN.to_string()), i64::MIN),
    ] {
        let created = create(id, caller(Some(claim.clone())))
            .await
            .unwrap_or_else(|e| panic!("claim {claim:?}: {e:?}"));
        assert_eq!(created.ownerE8, BigInt::new(expected), "claim {claim:?}");
        let stored: i64 =
            cratestack::sqlx::query_scalar("SELECT owner_e8 FROM bi_pol_stampeds WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .expect("stored");
        assert_eq!(stored, expected, "claim {claim:?}: stored value");
    }

    // A claim that is not a canonical BigInt is refused, never stored.
    for (id, claim) in [
        (7_i64, Value::String("007".to_owned())),
        (8, Value::String("+5".to_owned())),
        (9, Value::String("9223372036854775808".to_owned())),
        (10, Value::Bool(true)),
        (11, Value::Float(5.0)),
    ] {
        let result = create(id, caller(Some(claim.clone()))).await;
        assert!(
            result.is_err(),
            "claim {claim:?} must be refused, got {result:?}"
        );
    }
    // No claim at all.
    assert!(create(12, caller(None)).await.is_err());
    let stored: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM bi_pol_stampeds")
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(
        stored, 6,
        "only the six valid claims may have written a row"
    );
}

// ---------------------------------------------------------------------------
// procedure policies (evaluated against the arguments through `Value`)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Procedures;

macro_rules! pair_procedures {
    ($($method:ident => $module:ident),* $(,)?) => {
        $(
            async fn $method(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: cratestack_schema::procedures::$module::Args,
                _authorized: cratestack_schema::procedures::$module::Authorized,
            ) -> Result<cratestack_schema::procedures::$module::Output, CratestackError> {
                Ok(BiPair { a: args.args.a, b: args.args.b })
            }
        )*
    };
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    pair_procedures! {
        bi_ne_lit => bi_ne_lit,
        bi_eq_lit => bi_eq_lit,
        bi_deny_eq => bi_deny_eq,
        bi_ne_auth => bi_ne_auth,
        bi_eq_auth => bi_eq_auth,
        bi_ne_input => bi_ne_input,
        bi_eq_input => bi_eq_input,
    }

    async fn bi_direct_ne(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: bi_direct_ne::Args,
        _authorized: bi_direct_ne::Authorized,
    ) -> Result<bi_direct_ne::Output, CratestackError> {
        Ok(BiPair {
            a: args.a,
            b: args.b,
        })
    }

    async fn bi_isolated_ne(
        &self,
        _db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        args: bi_isolated_ne::Args,
        _authorized: bi_isolated_ne::Authorized,
    ) -> Result<bi_isolated_ne::Output, CratestackError> {
        Ok(BiPair {
            a: args.args.a,
            b: args.args.b,
        })
    }

    async fn bi_direct_ne_auth(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: bi_direct_ne_auth::Args,
        _authorized: bi_direct_ne_auth::Authorized,
    ) -> Result<bi_direct_ne_auth::Output, CratestackError> {
        Ok(BiPair {
            a: args.a,
            b: args.b,
        })
    }
}

fn router(pool: &PgPool) -> Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        header_auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

/// `(procedure, claim headers, a, b, allowed)`
type Case = (&'static str, Vec<(&'static str, String)>, i64, i64, bool);

fn quota(n: i64) -> Vec<(&'static str, String)> {
    vec![("x-quota-int", n.to_string())]
}

fn procedure_cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    // `args.a != 100`
    for (a, allowed) in [
        (100, false),
        (101, true),
        (0, true),
        (A53P1, true),
        (i64::MAX, true),
        (i64::MIN, true),
    ] {
        cases.push(("biNeLit", vec![], a, 0, allowed));
    }
    // `args.a != 100` under `@isolation("serializable")`
    for (a, allowed) in [
        (100, false),
        (101, true),
        (A53P1, true),
        (i64::MAX, true),
        (i64::MIN, true),
    ] {
        cases.push(("biIsolatedNe", vec![], a, 0, allowed));
    }
    // `args.a == 100`
    for (a, allowed) in [(100, true), (101, false), (A53P1, false), (i64::MAX, false)] {
        cases.push(("biEqLit", vec![], a, 0, allowed));
    }
    // `@allow(auth() != null)` `@deny(args.a == 100)`
    for (a, allowed) in [(100, false), (101, true), (i64::MAX, true), (A53P1, true)] {
        cases.push(("biDenyEq", vec![], a, 0, allowed));
    }
    // `args.a != auth().quotaE8`
    for (claim, a, allowed) in [
        (A53P1, A53P1, false),
        (A53P1, A53, true),
        (A53P1, A53P2, true),
        (i64::MAX, i64::MAX, false),
        (i64::MAX, A53P1, true),
        (i64::MIN, i64::MIN, false),
        (i64::MIN, 0, true),
    ] {
        cases.push(("biNeAuth", quota(claim), a, 0, allowed));
    }
    // no claim: neither side can be asserted, so it denies
    cases.push(("biNeAuth", vec![], 5, 0, false));
    // `args.a == auth().quotaE8`
    for (claim, a, allowed) in [
        (A53P1, A53P1, true),
        (A53P1, A53, false),
        (i64::MAX, i64::MAX, true),
        (i64::MAX, i64::MAX - 1, false),
        (i64::MIN, i64::MIN, true),
        (i64::MIN, i64::MIN + 1, false),
    ] {
        cases.push(("biEqAuth", quota(claim), a, 0, allowed));
    }
    // `args.a != args.b`
    for (a, b, allowed) in [
        (A53P1, A53P1, false),
        (i64::MAX, i64::MAX, false),
        (i64::MIN, i64::MIN, false),
        (0, 0, false),
        (A53P1, A53, true),
        (i64::MAX, i64::MIN, true),
        (A53, A53P1, true),
    ] {
        cases.push(("biNeInput", vec![], a, b, allowed));
    }
    // `args.a == args.b`
    for (a, b, allowed) in [
        (A53P1, A53P1, true),
        (i64::MAX, i64::MAX, true),
        (i64::MIN, i64::MIN, true),
        (A53P1, A53, false),
        (i64::MAX, i64::MIN, false),
        (i64::MAX, i64::MAX - 1, false),
    ] {
        cases.push(("biEqInput", vec![], a, b, allowed));
    }
    // a bare BigInt argument
    for (a, allowed) in [(100, false), (101, true), (A53P1, true), (i64::MAX, true)] {
        cases.push(("biDirectNe", vec![], a, 1, allowed));
    }
    for (claim, a, allowed) in [
        (A53P1, A53P1, false),
        (A53P1, 5, true),
        (i64::MAX, i64::MAX, false),
    ] {
        cases.push(("biDirectNeAuth", quota(claim), a, 1, allowed));
    }
    cases
}

#[tokio::test]
async fn procedure_policies_on_a_bigint_argument_deny_what_they_say() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let router = router(&test_pg.pool);
    let mut failures: Vec<String> = Vec::new();
    for wire in Wire::BOTH {
        for (procedure, claim, a, b, allowed) in procedure_cases() {
            let body = if procedure.starts_with("biDirect") {
                json!({"a": a.to_string(), "b": b.to_string()})
            } else {
                json!({"args": {"a": a.to_string(), "b": b.to_string()}})
            };
            let headers: Vec<(&str, &str)> = claim.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let reply = call(
                &router,
                "POST",
                &format!("/$procs/{procedure}"),
                wire,
                &headers,
                Some(&body),
            )
            .await;
            let want = if allowed {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            };
            if reply.status != want {
                failures.push(format!(
                    "{wire:?} {procedure} claim={claim:?} a={a} b={b}: expected {want}, got {} {}{}",
                    reply.status,
                    reply.json(),
                    if allowed { "" } else { "  <- ALLOWED: fails open" }
                ));
            } else if allowed {
                let out: &Json = &reply.json();
                if out["a"] != Json::String(a.to_string()) {
                    failures.push(format!("{wire:?} {procedure}: echoed a = {out}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} procedure policy case(s) wrong:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The procedure counterpart of the string-claim test above: undecidable, so
/// both `==` and `!=` against `auth().quotaE8` deny, whether or not the
/// argument equals the claim's number.
#[tokio::test]
async fn a_string_claim_in_a_procedure_policy_comparison_is_undecidable_and_denies_both_ways() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let router = router(&test_pg.pool);
    let mut failures: Vec<String> = Vec::new();
    for wire in Wire::BOTH {
        for procedure in ["biNeAuth", "biEqAuth", "biDirectNeAuth"] {
            for a in [ABOVE_2_53, "5"] {
                let body = if procedure == "biDirectNeAuth" {
                    json!({"a": a, "b": "1"})
                } else {
                    json!({"args": {"a": a, "b": "1"}})
                };
                let reply = call(
                    &router,
                    "POST",
                    &format!("/$procs/{procedure}"),
                    wire,
                    &[("x-quota-str", ABOVE_2_53)],
                    Some(&body),
                )
                .await;
                if reply.status != StatusCode::FORBIDDEN {
                    failures.push(format!(
                        "{wire:?} {procedure} a={a} with the string claim \"{ABOVE_2_53}\": \
                         expected 403, got {}",
                        reply.status
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
