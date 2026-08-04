//! cratestack#398 regression test — the exact reproduction from the
//! issue: `include_server_schema!` used to emit a struct field named
//! after a Rust keyword verbatim (no raw-identifier escaping), so a
//! schema field named `match`/`type`/`ref`/... produced uncompilable
//! Rust with an error pointing at this file's `include_server_schema!`
//! line, naming no field.
//!
//! `keyword_fields_struct_compiles_and_constructs` needs no live
//! Postgres — like `type_block_model_reference.rs`, merely *compiling*
//! this file (constructing the generated input/model structs and calling
//! the generated `FieldRef` accessor functions) is the real proof: before
//! the fix, `cargo check`/`cargo test` on this crate failed outright.
//! `keyword_fields_round_trip_through_generated_orm` additionally proves
//! the fix against a real Postgres (skips without one — see
//! `support::pg`).
//!
//! `self`/`Self`/`super`/`crate` are deliberately not in this fixture —
//! the parser now rejects those at schema-parse time; see
//! `cratestack_parser::tests_reserved_keywords`.

use cratestack::sqlx::query;
use cratestack::{CoolContext, Value, include_client_schema, include_server_schema};

include_server_schema!("tests/fixtures/keyword_fields.cstack", db = Postgres);

mod support;
use support::pg;

fn build_input(id: i64) -> cratestack_schema::CreateKeywordFieldsInput {
    cratestack_schema::CreateKeywordFieldsInput {
        id,
        r#match: "match-value".to_owned(),
        r#type: "type-value".to_owned(),
        r#ref: "ref-value".to_owned(),
        r#move: "move-value".to_owned(),
        r#impl: "impl-value".to_owned(),
        r#fn: "fn-value".to_owned(),
        r#let: "let-value".to_owned(),
        r#loop: "loop-value".to_owned(),
        r#box: "box-value".to_owned(),
    }
}

/// No DB required: constructing the generated `Create...Input` (a raw
/// identifier at every field), plus resolving the `FieldRef` accessor
/// function for the `match`-named field, is already the compile-time
/// proof this issue is about.
#[test]
fn keyword_fields_struct_compiles_and_constructs() {
    let input = build_input(1);
    assert_eq!(input.r#match, "match-value");
    assert_eq!(input.r#type, "type-value");
    assert_eq!(input.r#box, "box-value");

    // The FieldRef accessor emission site — a function literally named
    // `r#match()` in the generated `keyword_fields` module.
    let field = cratestack_schema::keyword_fields::r#match();
    let _filter = field.eq("needle".to_owned());
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS keyword_fields")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        r#"CREATE TABLE keyword_fields (
            id BIGINT PRIMARY KEY,
            "match" TEXT NOT NULL,
            "type" TEXT NOT NULL,
            "ref" TEXT NOT NULL,
            "move" TEXT NOT NULL,
            "impl" TEXT NOT NULL,
            "fn" TEXT NOT NULL,
            "let" TEXT NOT NULL,
            "loop" TEXT NOT NULL,
            "box" TEXT NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .expect("create keyword_fields table");
}

#[tokio::test]
async fn keyword_fields_round_trip_through_generated_orm() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    let input = build_input(1);

    cool.keyword_fields()
        .create(input.clone())
        .run(&ctx)
        .await
        .expect("create with every keyword-named field must succeed");

    let fetched = cool
        .keyword_fields()
        .find_unique(1)
        .run(&ctx)
        .await
        .expect("find_unique must succeed")
        .expect("row exists");

    assert_eq!(fetched.r#match, input.r#match);
    assert_eq!(fetched.r#type, input.r#type);
    assert_eq!(fetched.r#ref, input.r#ref);
    assert_eq!(fetched.r#move, input.r#move);
    assert_eq!(fetched.r#impl, input.r#impl);
    assert_eq!(fetched.r#fn, input.r#fn);
    assert_eq!(fetched.r#let, input.r#let);
    assert_eq!(fetched.r#loop, input.r#loop);
    assert_eq!(fetched.r#box, input.r#box);
}

/// The Rust CLIENT codegen path (`include_client_schema!`) reuses the
/// same struct-field emitter as the server path (see
/// `cratestack_macros::model::struct_only::generate_client_model_struct`),
/// but it's a genuinely separate macro entry point — prove it compiles
/// too, not just infer it from the server-side test above.
mod client_only_schema {
    use super::include_client_schema;

    include_client_schema!("tests/fixtures/keyword_fields.cstack");

    #[test]
    fn keyword_fields_client_struct_compiles_and_constructs() {
        let input = cratestack_schema::CreateKeywordFieldsInput {
            id: 1,
            r#match: "match-value".to_owned(),
            r#type: "type-value".to_owned(),
            r#ref: "ref-value".to_owned(),
            r#move: "move-value".to_owned(),
            r#impl: "impl-value".to_owned(),
            r#fn: "fn-value".to_owned(),
            r#let: "let-value".to_owned(),
            r#loop: "loop-value".to_owned(),
            r#box: "box-value".to_owned(),
        };
        assert_eq!(input.r#match, "match-value");
        assert_eq!(input.r#box, "box-value");
    }
}
