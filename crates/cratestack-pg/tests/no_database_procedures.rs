//! cratestack#327 regression: `datasource { provider = "none" }` paired
//! with `db = None` compiles cleanly for a procedures-only, zero-model
//! schema — the positive half of the datasource/macro-argument cross-check
//! (`crates/cratestack-macros/src/include/datasource_guard.rs`). The
//! negative half (a mismatch failing to compile) is demonstrated manually
//! per this story's PR description, following the same precedent as
//! `reject_grpc.rs`/`parse.rs`'s composite-PK guard: a `proc_macro::TokenStream`
//! compile-error path can't be exercised from a plain `cargo test` run.
//!
//! `Cratestack::builder(pool)` still takes a `sqlx::PgPool` here — codegen
//! changes to that signature for the no-database mode are explicitly out of
//! scope for this story (see the epic's later stories).

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CoolContext, CoolError};

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse without opening a socket");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[test]
fn no_database_schema_declares_zero_models_and_one_procedure() {
    assert_eq!(cratestack_schema::MODEL_COUNT, 0);
    assert_eq!(cratestack_schema::PROCEDURE_COUNT, 1);
    assert_eq!(cratestack_schema::TRANSPORT_STYLE, "rest");
}

#[tokio::test]
async fn no_database_schema_procedure_handler_still_dispatches() {
    let db = test_db();
    let procedures = Procedures;
    let output = cratestack_schema::procedures::ProcedureRegistry::ping(
        &procedures,
        &db,
        &CoolContext::anonymous(),
        cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs {
                message: "hello".to_owned(),
            },
        },
    )
    .await
    .expect("ping handler should succeed");

    assert_eq!(output.echo, "hello");
}
