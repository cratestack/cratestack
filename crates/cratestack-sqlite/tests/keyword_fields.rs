//! cratestack#398 regression test: a schema field named after a Rust
//! keyword used to make `include_embedded_schema!` (and
//! `include_server_schema!`) emit uncompilable Rust — the generated struct
//! field was written verbatim, with no raw-identifier (`r#`) escaping.
//!
//! This is the real reproduction from the issue: before the fix, this
//! file failed to compile at all, with `rustc` pointing at the
//! `include_embedded_schema!` line below rather than at any one field.
//! After the fix, `cratestack_macros::shared::ident` escapes every
//! keyword-named field as a raw identifier at every emission site (struct
//! field, `FromRow`/row-decode, `FieldRef` accessor, client struct), so
//! this compiles and round-trips like any other field.
//!
//! `self`/`Self`/`super`/`crate` are deliberately not included here — the
//! parser now rejects those outright at schema-parse time (no raw
//! identifier form exists for them at all); see
//! `cratestack_parser::tests_reserved_keywords`.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/keyword_fields.cstack");

use cratestack_schema::KEYWORD_FIELDS_MODEL;
use cratestack_schema::models::KeywordFields;

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&KEYWORD_FIELDS_MODEL))
                .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn every_keyword_named_field_compiles_and_round_trips() {
    let runtime = setup();
    let delegate = ModelDelegate::<KeywordFields, i64>::new(&runtime, &KEYWORD_FIELDS_MODEL);

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

    let created = delegate
        .create(input.clone())
        .run()
        .expect("create with every keyword-named field must succeed");

    let fetched = delegate
        .find_unique(1)
        .run()
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
    assert_eq!(fetched, created);
}

#[test]
fn keyword_field_ref_accessors_compile_and_filter() {
    let runtime = setup();
    let delegate = ModelDelegate::<KeywordFields, i64>::new(&runtime, &KEYWORD_FIELDS_MODEL);

    delegate
        .create(cratestack_schema::CreateKeywordFieldsInput {
            id: 1,
            r#match: "needle".to_owned(),
            r#type: "type-value".to_owned(),
            r#ref: "ref-value".to_owned(),
            r#move: "move-value".to_owned(),
            r#impl: "impl-value".to_owned(),
            r#fn: "fn-value".to_owned(),
            r#let: "let-value".to_owned(),
            r#loop: "loop-value".to_owned(),
            r#box: "box-value".to_owned(),
        })
        .run()
        .expect("create must succeed");

    // The generated `FieldRef` accessor for a `match`-named field is itself
    // a function named `r#match()` — proving the DSL accessor emission
    // site (not just the struct field) is escaped too.
    let found = delegate
        .find_many()
        .where_(cratestack_schema::keyword_fields::r#match().eq("needle".to_owned()))
        .run()
        .expect("filtering on a keyword-named field must succeed");
    assert_eq!(found.len(), 1);
}
