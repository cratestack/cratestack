//! SQLite/embedded mirror of `cratestack-pg`'s round-trip coverage test
//! for cratestack#232 — see that crate's `tests/round_trip_types.rs` and
//! this crate's `tests/fixtures/round_trip_types.cstack` for the shared
//! rationale. Here the "real DDL" comes from `create_table_sql`, the
//! runtime's own bootstrap path — there is no `cratestack-migrate` step
//! on embedded (see the `cratestack_rusqlite::ddl` module docs).
//!
//! Per issue #232 §4, SQLite never had the enum-storage disagreement
//! #228 hit on Postgres (every column shares BLOB affinity here,
//! including enum columns — see `emit/sqlite/columns.rs`), so both
//! models below are expected to pass on `main` as-is, unlike the
//! Postgres mirror's enum test.

use std::collections::BTreeSet;
use std::str::FromStr;

use cratestack::include_embedded_schema;
use cratestack::{Decimal, Json, RusqliteRuntime, Value};
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/round_trip_types.cstack");

use cratestack_schema::models::{RoundTripEnum, RoundTripScalar};
use cratestack_schema::{ROUND_TRIP_ENUM_MODEL, ROUND_TRIP_SCALAR_MODEL};

/// Scalar builtins this fixture covers — kept in lockstep with
/// `crates/cratestack-pg/tests/round_trip_types.rs`'s constant of the
/// same name (see that file for the full rationale on the guard test
/// below and on why `jsonReq` isn't present in this backend's fixture).
const COVERED_SCALAR_TYPES: &[&str] = &[
    "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Decimal", "Json", "Bytes", "Uuid",
];

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = format!(
        "{};\n{};",
        create_table_sql(&ROUND_TRIP_SCALAR_MODEL),
        create_table_sql(&ROUND_TRIP_ENUM_MODEL),
    );
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn covered_scalar_types_match_parser_builtin_type_names_minus_page() {
    // `Page`/`PageInput` aren't model-field-storable scalars — `Page<T>` is
    // restricted to procedure return types and `PageInput` to procedure
    // argument types, neither round-trippable through a model's own ORM
    // columns — so both are excluded here, same as `Page` already was.
    // `Vector(n)` (see `docs/design/extensions.md` §6, cratestack#155) is
    // excluded for a stronger reason than "not covered yet": pgvector is a
    // Postgres-only extension, and `include_embedded_schema!` rejects
    // `extension pgvector { }` unconditionally, regardless of any Cargo
    // feature (cratestack#161's `guard_embedded_declared_extensions`) — a
    // `Vector(n)` field can never exist on this rusqlite backend at all,
    // so there is no DDL/round-trip coverage to add here, ever.
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

#[test]
// `decimal.clone()` below (not `decimal` twice): pre-existing bug, unrelated
// to cratestack#505 — `bigdecimal::BigDecimal` isn't `Copy` (unlike
// `rust_decimal::Decimal`), so this test only compiled under the default
// `decimal-rust-decimal` backend before. Never caught by
// `.ci/feature-matrix.sh`, which only runs a plain `cargo check` (no
// `--all-targets`) for `cratestack-sqlite`. The `.clone()` itself then
// trips `clippy::clone_on_copy` under `decimal-rust-decimal` (where
// `Decimal` happens to be `Copy`) — same rationale as `cratestack-sqlx`'s
// `push_bind_value`.
#[allow(clippy::clone_on_copy)]
fn all_non_enum_builtin_scalars_round_trip_through_generated_orm() {
    let runtime = setup();
    let delegate = ModelDelegate::<RoundTripScalar, i64>::new(&runtime, &ROUND_TRIP_SCALAR_MODEL);

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
        decimalReq: decimal.clone(),
        decimalOpt: Some(decimal),
        jsonOpt: Some(Json(Value::String("optional-payload".to_owned()))),
        bytesReq: vec![0xDE, 0xAD, 0xBE, 0xEF],
        bytesOpt: Some(vec![0xCA, 0xFE]),
        uuidReq: uuid,
        uuidOpt: Some(uuid),
    };

    let created = delegate
        .create(input.clone())
        .run()
        .expect("create via the generated Create...Input must succeed against runtime DDL");

    let fetched = delegate
        .find_unique(1)
        .run()
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
    assert_eq!(fetched.jsonOpt, input.jsonOpt);
    assert_eq!(fetched.bytesReq, input.bytesReq);
    assert_eq!(fetched.bytesOpt, input.bytesOpt);
    assert_eq!(fetched.uuidReq, input.uuidReq);
    assert_eq!(fetched.uuidOpt, input.uuidOpt);
    assert_eq!(fetched, created);
}

#[test]
fn optional_builtin_scalars_round_trip_as_null() {
    let runtime = setup();
    let delegate = ModelDelegate::<RoundTripScalar, i64>::new(&runtime, &ROUND_TRIP_SCALAR_MODEL);

    let date_time = chrono::DateTime::parse_from_rfc3339("2026-07-29T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let decimal = Decimal::from_str("1").unwrap();
    let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

    delegate
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
            jsonOpt: None,
            bytesReq: vec![0x01],
            bytesOpt: None,
            uuidReq: uuid,
            uuidOpt: None,
        })
        .run()
        .expect("create with every optional field absent must succeed");

    let fetched = delegate
        .find_unique(2)
        .run()
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

#[test]
fn declared_enum_round_trips_through_generated_orm() {
    let runtime = setup();
    let delegate = ModelDelegate::<RoundTripEnum, i64>::new(&runtime, &ROUND_TRIP_ENUM_MODEL);

    let created = delegate
        .create(cratestack_schema::CreateRoundTripEnumInput {
            id: 1,
            statusReq: cratestack_schema::RoundTripStatus::Active,
            statusOpt: Some(cratestack_schema::RoundTripStatus::Inactive),
        })
        .run()
        .expect("create of a declared-enum column must succeed on the embedded backend");

    let fetched = delegate
        .find_unique(1)
        .run()
        .expect("find_unique of a declared-enum column must succeed")
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
