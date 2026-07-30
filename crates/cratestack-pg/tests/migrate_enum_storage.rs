//! End-to-end coverage for issues #227 and #228 against a real Postgres.
//!
//! Both bugs were invisible to `cratestack-cli check` and to every
//! existing test, because each side was individually valid: the
//! emitter produced syntactically correct DDL, and the generated row
//! decoder used a normal `sqlx` pattern. They only met at runtime.
//!
//! This test closes that gap by driving the *whole* path — emit the
//! migration, apply it to a real database, write a row, and read it
//! back **through the macro-generated ORM decoder**:
//!
//! * **#227** — `@default(Device)` is a bareword in `.cstack`. Emitted
//!   unquoted it becomes `DEFAULT Device`, which Postgres parses as a
//!   column reference and rejects at `CREATE TABLE` time. The
//!   migration below simply would not apply.
//! * **#228** — the emitter used to type enum columns as a native
//!   `CREATE TYPE ... AS ENUM`, while the generated decoder reads every
//!   enum field with `try_get::<String>`. Every read failed with
//!   `sqlx::Error::Decode`. Reading back through the generated model
//!   (rather than a raw `query()`) is the part that actually catches
//!   this — a raw read would not.
//!
//! It also covers the migration path that replaces
//! `ALTER TYPE ... ADD VALUE`: adding a variant must produce a working
//! migration, so the test applies a second generated migration and then
//! stores and reads back a value that only exists in the new variant set.

use cratestack::sqlx::query;
use cratestack::{CoolContext, Migration, Value, apply_pending, include_server_schema};
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::{parse_schema, parse_schema_file};

include_server_schema!("tests/fixtures/migrate_enum_storage.cstack", db = Postgres);

use cratestack_schema::PrincipalType;

mod support;

use support::pg;

const FIXTURE: &str = "tests/fixtures/migrate_enum_storage.cstack";

/// The same schema as the fixture, minus the `Service` variant. Diffing
/// this against the fixture is what produces the add-a-variant
/// migration.
const SCHEMA_V1: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

enum PrincipalType {
  Device
  Person
}

model Principal {
  id Int @id
  kind PrincipalType @default(Device)
  role PrincipalType
  fallback PrincipalType?

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}
"#;

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, principals")
        .execute(pool)
        .await
        .expect("drop");
    // Left behind by a pre-fix run of this test, and by any database
    // migrated with the old native-enum emitter.
    query("DROP TYPE IF EXISTS principal_type")
        .execute(pool)
        .await
        .expect("drop legacy enum type");
}

#[tokio::test]
async fn enum_columns_round_trip_through_the_generated_decoder() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let empty = parse_schema("").expect("empty schema should parse");
    let v1 = parse_schema(SCHEMA_V1).expect("v1 schema should parse");
    let v2 = parse_schema_file(FIXTURE).expect("fixture schema should parse");

    let initial = postgres::emit(&diff(&empty, &v1));
    let add_variant = postgres::emit(&diff(&v1, &v2));

    // ---- #228: TEXT storage, not a native enum type ----------------
    assert!(
        !initial.up.contains("CREATE TYPE"),
        "enum columns must not be typed as a native Postgres enum; up was: {}",
        initial.up
    );
    assert!(
        initial.up.contains("kind TEXT NOT NULL"),
        "up was: {}",
        initial.up
    );

    // ---- #227: the bareword default must be quoted -----------------
    assert!(
        initial.up.contains("DEFAULT 'Device'"),
        "up was: {}",
        initial.up
    );
    assert!(
        !initial.up.contains("DEFAULT Device"),
        "an unquoted bareword default parses as a column reference; up was: {}",
        initial.up
    );

    // ---- the add-a-variant path replaces ALTER TYPE ADD VALUE ------
    assert!(
        !add_variant.up.contains("ALTER TYPE"),
        "up was: {}",
        add_variant.up
    );
    assert!(
        add_variant
            .up
            .contains("ADD CONSTRAINT principals_role_enum_check"),
        "adding a variant must rebuild the membership CHECK; up was: {}",
        add_variant.up
    );

    // Both migrations must apply cleanly. Pre-#227 the first one failed
    // here with "cannot use column reference in DEFAULT expression".
    apply_pending(
        pool,
        &[
            Migration {
                id: "20260730000001_principals_init".to_owned(),
                description: "principals with enum columns".to_owned(),
                up: initial.up.clone(),
                down: None,
            },
            Migration {
                id: "20260730000002_principals_add_service".to_owned(),
                description: "add the Service variant".to_owned(),
                up: add_variant.up.clone(),
                down: None,
            },
        ],
    )
    .await
    .expect("emitted DDL must apply cleanly against real Postgres");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // Write a row using `Service` — a variant that only exists because
    // the second migration rebuilt the CHECK. If that migration were
    // wrong, this INSERT would trip the constraint.
    let created = cool
        .principal()
        .create(cratestack_schema::CreatePrincipalInput {
            id: 1,
            role: PrincipalType::Service,
            fallback: Some(PrincipalType::Person),
        })
        .run(&ctx())
        .await
        .expect("create must round-trip the enum columns");

    assert_eq!(created.role, PrincipalType::Service);
    assert_eq!(created.fallback, Some(PrincipalType::Person));
    // `kind` was never sent — this value came from the DDL DEFAULT.
    assert_eq!(created.kind, PrincipalType::Device);

    // The read that #228 made impossible: decoding an enum column back
    // out through the generated `FromRow`. Against a native enum column
    // this failed with `sqlx::Error::Decode: mismatched types; Rust type
    // `alloc::string::String` (as SQL type `TEXT`) is not compatible
    // with SQL type `principal_type``.
    let found = cool
        .principal()
        .find_unique(1)
        .run(&ctx())
        .await
        .expect("read must decode the enum columns")
        .expect("row should exist");

    assert_eq!(found.role, PrincipalType::Service);
    assert_eq!(found.kind, PrincipalType::Device);
    assert_eq!(found.fallback, Some(PrincipalType::Person));

    // A NULL enum column must decode as `None`, not error.
    let with_null = cool
        .principal()
        .create(cratestack_schema::CreatePrincipalInput {
            id: 2,
            role: PrincipalType::Device,
            fallback: None,
        })
        .run(&ctx())
        .await
        .expect("create with a NULL enum column");
    assert_eq!(with_null.fallback, None);
}

/// The `CHECK` constraint has to actually do the job the native enum
/// type used to do — otherwise TEXT storage would be a silent
/// downgrade in validation.
#[tokio::test]
async fn membership_check_rejects_values_outside_the_variant_set() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let empty = parse_schema("").expect("empty schema should parse");
    let v2 = parse_schema_file(FIXTURE).expect("fixture schema should parse");
    let migration = postgres::emit(&diff(&empty, &v2));

    apply_pending(
        pool,
        &[Migration {
            id: "20260730000001_principals_init".to_owned(),
            description: "principals with enum columns".to_owned(),
            up: migration.up.clone(),
            down: None,
        }],
    )
    .await
    .expect("emitted DDL must apply cleanly");

    // Bypass the ORM — this is the database-level guarantee.
    let rejected = query(
        "INSERT INTO principals (id, kind, role, fallback) \
         VALUES (1, 'Device', 'Wanderer', NULL)",
    )
    .execute(pool)
    .await;
    let error = rejected.expect_err("a value outside the variant set must be rejected");
    assert!(
        error.to_string().contains("principals_role_enum_check"),
        "expected the enum membership CHECK to fire, got: {error}",
    );

    // And a legitimate value still goes in.
    query(
        "INSERT INTO principals (id, kind, role, fallback) \
         VALUES (3, 'Person', 'Service', 'Device')",
    )
    .execute(pool)
    .await
    .expect("a valid value must be accepted");
}
