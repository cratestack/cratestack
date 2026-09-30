//! EXT-14 (cratestack#1123), the compatible-contract lock on a *model's*
//! ops, end to end and without a live database.
//!
//! Models cannot live under `db = None`, so this uses the Postgres facade
//! with a pool that connects lazily to a closed port: nothing here needs a
//! row. What it needs is the envelope layer and the decode in front of the
//! database, and those are observable without one. A request the layer
//! refuses never reaches the handler (`426`, or `401`); one it accepts is
//! decoded, authorised and only then fails to reach Postgres, which the
//! client sees as a *sealed* server error. So, for an old client against a
//! server whose `Note` gained an optional field:
//!
//! - with the lock, `create` gets a sealed error (verified under the old
//!   digest: opened; the new optional field decoded as absent, because a
//!   decode failure would be a `400`) and not `ContractUnsupported`;
//! - without the lock, `create` is the unsigned `426`.
//!
//! The end-to-end success path of a locked model op (a row written and read
//! back) needs the live `just test-live` Postgres; the layer's own tests
//! (`envelope_layer/tests/contracts_history.rs`) cover a two-digest row with
//! a REST key, and `cratestack-api`'s `cose_envelope_contract_lock.rs`
//! covers a full success for procedures.

#[path = "../../cratestack-api/tests/cose_client_support/mod.rs"]
mod cose_client_support;

use std::time::Duration;

use cose_client_support::{AUDIENCE, Kind, client_envelope, layer, runtime, serve};
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_client_rust::{CborCodec, EnvelopeError, RpcClientError};

mod old {
    cratestack::include_client_schema!("tests/fixtures/contract_lock_models_old.cstack");
}

mod locked {
    cratestack::include_server_schema!(
        "tests/fixtures/contract_lock_models_new.cstack",
        db = Postgres,
        contracts = "tests/fixtures/contract_lock_models_new.contracts.lock"
    );
}

mod unlocked {
    cratestack::include_server_schema!(
        "tests/fixtures/contract_lock_models_new.cstack",
        db = Postgres
    );
}

#[derive(Clone)]
struct NoProcedures;
impl locked::cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}
impl unlocked::cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
    type Error = CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

/// A pool that has never connected and cannot: port 1 refuses at once.
fn dead_pool() -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(2))
        .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/nowhere")
        .expect("a lazy pool parses its URL")
}

macro_rules! old_client_of {
    ($module:ident) => {{
        use $module::cratestack_schema as srv;
        let router = srv::axum::rpc_router(
            srv::Cratestack::builder(dead_pool()).build(),
            NoProcedures,
            (),
            cratestack_codec_cbor::CborCodec,
            PassThroughAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
        .layer(layer(Kind::Ed25519, |envelope, policy, audience| {
            srv::axum::envelope_layer(envelope, policy, audience)
        }));
        let addr = serve(router).await;
        old::cratestack_schema::client::Client::<CborCodec>::new(runtime(
            addr,
            client_envelope(Kind::Ed25519, AUDIENCE),
        ))
    }};
}

fn input() -> old::cratestack_schema::CreateNoteInput {
    old::cratestack_schema::CreateNoteInput {
        id: 1,
        body: "hello".to_owned(),
    }
}

fn accepted(table: &'static [(&str, &[[u8; 32]])], op: &str) -> usize {
    table
        .iter()
        .find(|(key, _)| *key == op)
        .expect("row")
        .1
        .len()
}

#[test]
fn every_note_op_keeps_the_shipped_digest_when_the_lock_holds_it() {
    for op in ["create", "get", "list", "update", "delete"] {
        let key = format!("model.Note.{op}");
        assert_eq!(
            accepted(locked::cratestack_schema::ACCEPTED_CONTRACTS, &key),
            2,
            "{key}: current, then the locked one"
        );
        assert_eq!(
            accepted(unlocked::cratestack_schema::ACCEPTED_CONTRACTS, &key),
            1,
            "{key}: no lock, current only"
        );
    }
}

#[tokio::test]
async fn a_locked_model_op_is_accepted_and_decoded_then_fails_at_the_database() {
    let client = old_client_of!(locked);
    match client.notes().create(&input()).await {
        // The layer verified the request under the old digest, the new
        // server decoded the old input (no `tag`), policy passed, and only
        // the pool failed. The error came back sealed and the client opened
        // it, under the digest it sent.
        Err(RpcClientError::Remote(remote)) => {
            assert!(remote.status.is_server_error(), "{remote:?}");
        }
        other => panic!("expected a sealed server error from the dead pool, got {other:?}"),
    }
}

#[tokio::test]
async fn without_the_lock_the_model_op_is_the_426() {
    let client = old_client_of!(unlocked);
    match client.notes().create(&input()).await {
        Err(RpcClientError::Envelope(EnvelopeError::ContractUnsupported { op })) => {
            assert_eq!(op, "model.Note.create");
        }
        other => panic!("expected ContractUnsupported, got {other:?}"),
    }
}
