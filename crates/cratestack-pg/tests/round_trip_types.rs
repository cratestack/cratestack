//! Priority-(1) fix for cratestack#232: a generated round-trip test that
//! exercises the real seam #228 fell through — emitter DDL + generated
//! `Create...Input` write path + generated decoder read path — for every
//! entry in `cratestack_parser::builtin_type_names()` (minus `Page`, which
//! is procedure-return-only, never a field type) crossed with
//! `{Required, Optional}` arity, plus a declared enum.
//!
//! Unlike every other `banking_*`/`policy_db_*` fixture in this crate, the
//! DDL applied here is never hand-written: `emitted_migration_up` parses
//! this test's own schema fixture and runs it through
//! `cratestack_migrate::diff` + `emit::postgres`, exactly the path
//! `cratestack migrate diff` uses in production. That is the gap #232
//! identified — every existing integration test hand-wrote DDL that
//! happened to already agree with the decoder, so the emitter's own
//! opinion was never actually tested against it.
//!
//! Split into two models (see the fixture's own header comment) so an
//! enum-storage bug (#228) can't mask coverage of the other scalars: as of
//! this writing `main` predates the open, unmerged #233 fix, so
//! `declared_enum_round_trips_through_generated_orm` below is marked
//! `#[ignore]` — run it explicitly (see that test's doc comment) and it
//! reproduces #228's exact error signature, which is the evidence this
//! PR's issue asked for; see the PR body's Verification section for the
//! before/after run. It is `#[ignore]`d rather than left to fail outright
//! because CI's `test-ci-db` shard sets `CRATESTACK_REQUIRE_DB=1` and
//! retries on failure (see `.github/workflows/ci.yml`) — a deterministic
//! failure there would make every future PR's CI red until #233 merges,
//! for a fact everyone already knows. Remove the `#[ignore]` in the same
//! commit that merges #233's fix.

use std::collections::BTreeSet;
use std::str::FromStr;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, Decimal, Json, Value};
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::parse_schema;

include_server_schema!("tests/fixtures/round_trip_types.cstack", db = Postgres);

mod support;
use support::pg;

/// The exact same bytes the macro parsed above, fed independently through
/// `diff` + `emit::postgres` so the DDL under test is the emitter's real
/// output, not a hand-authored guess at what it would be.
const SCHEMA_SRC: &str = include_str!("fixtures/round_trip_types.cstack");

/// Scalar builtins this fixture covers — every entry in
/// `cratestack_parser::builtin_type_names()` except `Page` (procedure-
/// return-only, structurally never a field type — see `validate_type_ref`
/// in `cratestack-parser`). Kept as a `const` so the guard test below
/// fails loudly the moment a new builtin scalar is added without adding a
/// field for it in the fixture and write/assert coverage for it here.
const COVERED_SCALAR_TYPES: &[&str] = &[
    "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Decimal", "Json", "Bytes", "Uuid",
];

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, round_trip_scalars, round_trip_enums")
        .execute(pool)
        .await
        .expect("drop tables");
    // The tables above are re-created by `apply_emitted_migration` on every
    // test, but a Postgres `CREATE TYPE ... AS ENUM` has no table-level
    // equivalent of `DROP TABLE`'s cascade — the enum type outlives a
    // `DROP TABLE` of the column that used it. Four tests in this file
    // share one external Postgres (see `support::pg`) and each calls
    // `apply_emitted_migration`, so without this the second test to run
    // hits "type \"round_trip_status\" already exists" instead of
    // exercising anything about #228/#232.
    query("DROP TYPE IF EXISTS round_trip_status")
        .execute(pool)
        .await
        .expect("drop enum type");
}

/// Parse the fixture independently, diff it against an empty schema, and
/// run it through the real Postgres emitter — this is the DDL applied
/// below, never a hand-written `CREATE TABLE`.
fn emitted_migration_up() -> String {
    let empty = parse_schema("").expect("empty schema parses");
    let next = parse_schema(SCHEMA_SRC).expect("round-trip fixture parses");
    postgres::emit(&diff(&empty, &next).expect("diff should succeed")).up
}

async fn apply_emitted_migration(pool: &cratestack::sqlx::PgPool) {
    let up = emitted_migration_up();
    cratestack::apply_pending(
        pool,
        &[cratestack::Migration {
            id: "20260729000000_round_trip_types".to_owned(),
            description: "round-trip type coverage fixture (cratestack#232)".to_owned(),
            up,
            down: None,
        }],
    )
    .await
    .expect("emitter-generated DDL must apply cleanly against real Postgres");
}

#[test]
fn covered_scalar_types_match_parser_builtin_type_names_minus_page() {
    // Guard against cratestack#232 recurring in reverse: if a new builtin
    // scalar is ever added to `BUILTIN_TYPES` without adding round-trip
    // coverage for it here, this fails the suite instead of silently
    // shipping an uncovered type. `Page`/`PageInput` aren't model-field-
    // storable scalars — `Page<T>` is restricted to procedure return
    // types and `PageInput` to procedure argument types, neither
    // round-trippable through a model's own ORM columns — so both are
    // excluded here, same as `Page` already was. `Vector(n)` (see
    // `docs/design/extensions.md` §6, cratestack#155) is a real,
    // storable model-field scalar, but it's excluded from *this*
    // always-on fixture because it needs both the `pgvector` Cargo
    // feature (this file has no `required-features = ["pgvector"]`,
    // unlike `pgvector_feature_forwarding.rs`) and a real Postgres
    // with the `vector` extension available — round-trip coverage for
    // it lives in `emit::postgres::tests::extensions` (DDL) and
    // `pgvector_feature_forwarding.rs` (macro codegen) instead.
    let builtin: BTreeSet<&str> = cratestack_parser::builtin_type_names()
        .iter()
        .copied()
        .filter(|name| {
            *name != "Page" && *name != "PageInput" && *name != "FindMany" && *name != "Vector"
        })
        .collect();
    let covered: BTreeSet<&str> = COVERED_SCALAR_TYPES.iter().copied().collect();
    assert_eq!(
        builtin, covered,
        "cratestack_parser::builtin_type_names() (minus `Page`/`PageInput`/`FindMany`/`Vector`) and \
         this test's COVERED_SCALAR_TYPES have drifted — add a field plus write/assert coverage \
         above for the new/removed type. See cratestack#232.",
    );
}

#[tokio::test]
async fn all_non_enum_builtin_scalars_round_trip_through_generated_orm() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    apply_emitted_migration(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let date_time = chrono::DateTime::parse_from_rfc3339("2026-07-29T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let decimal = Decimal::from_str("1234.5678").unwrap();
    let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

    let input = cratestack_schema::CreateRoundTripScalarInput {
        id: 1,
        stringReq: "hello".to_owned(),
        stringOpt: Some("optional-hello".to_owned()),
        cuidReq: "cliqzroundtrip0001".to_owned(),
        cuidOpt: Some("cliqzroundtrip0002".to_owned()),
        intReq: 42,
        intOpt: Some(-7),
        floatReq: 3.5,
        floatOpt: Some(-2.25),
        booleanReq: true,
        booleanOpt: Some(false),
        dateTimeReq: date_time,
        dateTimeOpt: Some(date_time),
        decimalReq: decimal,
        decimalOpt: Some(decimal),
        jsonReq: Json(Value::String("required-payload".to_owned())),
        jsonOpt: Some(Json(Value::String("optional-payload".to_owned()))),
        bytesReq: vec![0xDE, 0xAD, 0xBE, 0xEF],
        bytesOpt: Some(vec![0xCA, 0xFE]),
        uuidReq: uuid,
        uuidOpt: Some(uuid),
    };

    let created = cool
        .round_trip_scalar()
        .create(input.clone())
        .run(&ctx())
        .await
        .expect(
            "create via the generated Create...Input must succeed against emitter-generated DDL",
        );

    let fetched = cool
        .round_trip_scalar()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("find_unique via the generated decoder must succeed")
        .expect("row exists");

    assert_eq!(fetched.stringReq, input.stringReq);
    assert_eq!(fetched.stringOpt, input.stringOpt);
    assert_eq!(fetched.cuidReq, input.cuidReq);
    assert_eq!(fetched.cuidOpt, input.cuidOpt);
    assert_eq!(fetched.intReq, input.intReq);
    assert_eq!(fetched.intOpt, input.intOpt);
    assert_eq!(fetched.floatReq, input.floatReq);
    assert_eq!(fetched.floatOpt, input.floatOpt);
    assert_eq!(fetched.booleanReq, input.booleanReq);
    assert_eq!(fetched.booleanOpt, input.booleanOpt);
    assert_eq!(fetched.dateTimeReq, input.dateTimeReq);
    assert_eq!(fetched.dateTimeOpt, input.dateTimeOpt);
    assert_eq!(fetched.decimalReq, input.decimalReq);
    assert_eq!(fetched.decimalOpt, input.decimalOpt);
    assert_eq!(fetched.jsonReq, input.jsonReq);
    assert_eq!(fetched.jsonOpt, input.jsonOpt);
    assert_eq!(fetched.bytesReq, input.bytesReq);
    assert_eq!(fetched.bytesOpt, input.bytesOpt);
    assert_eq!(fetched.uuidReq, input.uuidReq);
    assert_eq!(fetched.uuidOpt, input.uuidOpt);
    assert_eq!(fetched, created);
}

#[tokio::test]
async fn optional_builtin_scalars_round_trip_as_null() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    apply_emitted_migration(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let date_time = chrono::DateTime::parse_from_rfc3339("2026-07-29T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let decimal = Decimal::from_str("1").unwrap();
    let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

    cool.round_trip_scalar()
        .create(cratestack_schema::CreateRoundTripScalarInput {
            id: 2,
            stringReq: "required".to_owned(),
            stringOpt: None,
            cuidReq: "cliqzroundtrip0003".to_owned(),
            cuidOpt: None,
            intReq: 1,
            intOpt: None,
            floatReq: 1.0,
            floatOpt: None,
            booleanReq: false,
            booleanOpt: None,
            dateTimeReq: date_time,
            dateTimeOpt: None,
            decimalReq: decimal,
            decimalOpt: None,
            jsonReq: Json(Value::String("required".to_owned())),
            jsonOpt: None,
            bytesReq: vec![0x01],
            bytesOpt: None,
            uuidReq: uuid,
            uuidOpt: None,
        })
        .run(&ctx())
        .await
        .expect("create with every optional field absent must succeed");

    let fetched = cool
        .round_trip_scalar()
        .find_unique(2)
        .run(&ctx())
        .await
        .expect("find_unique must succeed")
        .expect("row exists");

    assert!(fetched.stringOpt.is_none());
    assert!(fetched.cuidOpt.is_none());
    assert!(fetched.intOpt.is_none());
    assert!(fetched.floatOpt.is_none());
    assert!(fetched.booleanOpt.is_none());
    assert!(fetched.dateTimeOpt.is_none());
    assert!(fetched.decimalOpt.is_none());
    assert!(fetched.jsonOpt.is_none());
    assert!(fetched.bytesOpt.is_none());
    assert!(fetched.uuidOpt.is_none());
}

/// The enum half — kept in its own model/test (see the fixture header) so
/// a storage disagreement here (#228) doesn't block the scalar coverage
/// above.
///
/// `#[ignore]`d until cratestack#233 (`fix/enum-text-storage-227-228`)
/// merges: on unpatched `main` this fails while applying the *second*
/// generated migration in this file's test binary with
/// `column "status_req" is of type round_trip_status but expression is of
/// type text` — the write-side twin of #228's reported read-side
/// `Decode` error, both symptoms of the same emitter/decoder disagreement
/// (see the module docs and the PR's Verification section for the full
/// before/after run, including confirmation this passes against #233's
/// branch). Run explicitly to reproduce:
/// `cargo test -p cratestack-pg --test round_trip_types -- --ignored
/// declared_enum_round_trips_through_generated_orm`.
#[tokio::test]
#[ignore = "blocked on cratestack#233 (enum TEXT+CHECK storage fix) landing; reproduces #228 as-is — see module docs"]
async fn declared_enum_round_trips_through_generated_orm() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    apply_emitted_migration(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let created = cool
        .round_trip_enum()
        .create(cratestack_schema::CreateRoundTripEnumInput {
            id: 1,
            statusReq: cratestack_schema::RoundTripStatus::Active,
            statusOpt: Some(cratestack_schema::RoundTripStatus::Inactive),
        })
        .run(&ctx())
        .await
        .expect(
            "create of a declared-enum column via the generated write path must succeed \
             against the emitter's own DDL — this is the write half of cratestack#228",
        );

    let fetched = cool
        .round_trip_enum()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect(
            "find_unique of a declared-enum column via the generated decoder must succeed \
             — this is the read half of cratestack#228",
        )
        .expect("row exists");

    assert_eq!(
        fetched.statusReq,
        cratestack_schema::RoundTripStatus::Active
    );
    assert_eq!(
        fetched.statusOpt,
        Some(cratestack_schema::RoundTripStatus::Inactive)
    );
    assert_eq!(fetched, created);
}
