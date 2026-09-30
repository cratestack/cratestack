//! EXT-14 (cratestack#1123), end to end: a client generated from the schema
//! as it shipped (`contract_old.cstack`) against a server generated from the
//! schema after an edit (`contract_new.cstack`), over a real listener with
//! the envelope layer in `Required` mode and a COSE-signed client.
//!
//! The edit is a policy, a validator, `@deprecated` and a new procedure
//! (none of them changes what goes over the wire) plus one wire-shape edit
//! (`retyped`'s argument). Under binding version 1 the first four refused
//! every old client, on every op, with an unsigned `401`. Under version 2
//! only `retyped` is refused, and with an answer the client can tell from
//! a bad key: the unsigned `426 contract_unsupported`.

mod cose_client_support;

use cose_client_support::{AUDIENCE, Kind, client_envelope, layer, runtime, serve};
use cratestack::{CratestackContext, CratestackError};
use cratestack_client_rust::{CborCodec, EnvelopeError, RpcClientError};

mod old {
    cratestack::include_client_schema!("tests/fixtures/contract_old.cstack");
}

mod new {
    cratestack::include_server_schema!("tests/fixtures/contract_new.cstack", db = None);
}

use new::cratestack_schema as srv;
use old::cratestack_schema as client_schema;

#[derive(Clone, Default)]
struct Procedures;

type Reply = Result<srv::PingReply, CratestackError>;

fn reply(echo: String) -> impl core::future::Future<Output = Reply> + Send {
    async move { Ok(srv::PingReply { echo }) }
}

impl srv::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::ping::Args,
        _authorized: srv::procedures::ping::Authorized,
    ) -> impl core::future::Future<Output = Reply> + Send {
        reply(format!("ping:{}", args.args.message))
    }

    fn retyped(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::retyped::Args,
        _authorized: srv::procedures::retyped::Authorized,
    ) -> impl core::future::Future<Output = Reply> + Send {
        reply(format!("retyped:{}", args.args.message))
    }

    fn untouched(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::untouched::Args,
        _authorized: srv::procedures::untouched::Authorized,
    ) -> impl core::future::Future<Output = Reply> + Send {
        reply(format!("untouched:{}", args.args.message))
    }

    fn brand_new(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::brand_new::Args,
        _authorized: srv::procedures::brand_new::Authorized,
    ) -> impl core::future::Future<Output = Reply> + Send {
        reply(format!("brand_new:{}", args.args.message))
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

type Client = client_schema::client::Client<CborCodec>;

async fn old_client_of_new_server() -> Client {
    let router = srv::axum::rpc_router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_cbor::CborCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer(Kind::Ed25519, |envelope, policy, audience| {
        srv::axum::envelope_layer(envelope, policy, audience)
    }));
    let addr = serve(router).await;
    Client::new(runtime(addr, client_envelope(Kind::Ed25519, AUDIENCE)))
}

/// The old client's `procedures::$op::Args` for `message`.
macro_rules! args {
    ($op:ident, $message:expr) => {
        client_schema::procedures::$op::Args {
            args: client_schema::PingArgs {
                message: $message.to_owned(),
            },
        }
    };
}

fn current(op: &str) -> [u8; 32] {
    let (_, accepted) = srv::ACCEPTED_CONTRACTS
        .iter()
        .find(|(key, _)| *key == op)
        .unwrap_or_else(|| panic!("the server has no {op}"));
    accepted[0]
}

fn shipped(op: &str) -> [u8; 32] {
    client_schema::OP_CONTRACTS
        .iter()
        .find(|(key, _)| *key == op)
        .unwrap_or_else(|| panic!("the old client has no {op}"))
        .1
}

#[test]
fn only_the_wire_shape_edit_moves_a_digest() {
    // The whole-IR identity, which binding version 1 bound, moved...
    assert_ne!(client_schema::SCHEMA_SHA256_BYTES, srv::SCHEMA_SHA256_BYTES);
    // ...but per op, only `retyped` did.
    for op in ["procedure.ping", "procedure.untouched"] {
        assert_eq!(shipped(op), current(op), "{op}");
    }
    assert_ne!(shipped("procedure.retyped"), current("procedure.retyped"));
    // A new op is a new row, and moves the whole-contract identity (which
    // only the signed `batch` binds).
    assert!(
        client_schema::OP_CONTRACTS
            .iter()
            .all(|(key, _)| *key != "procedure.brandNew")
    );
    assert_eq!(current("procedure.brandNew").len(), 32);
    assert_ne!(
        client_schema::CLIENT_CONTRACT_SHA256_BYTES,
        srv::CLIENT_CONTRACT_SHA256_BYTES
    );
    assert_ne!(shipped("batch"), current("batch"));
}

#[tokio::test]
async fn an_old_client_survives_server_only_edits() {
    let client = old_client_of_new_server().await;
    let ping = client.procedures().ping(&args!(ping, "a")).await;
    assert_eq!(ping.expect("policy and validator edits").echo, "ping:a");
    let untouched = client.procedures().untouched(&args!(untouched, "b")).await;
    assert_eq!(untouched.expect("@deprecated").echo, "untouched:b");
}

#[tokio::test]
async fn a_wire_shape_edit_is_the_unsigned_426_for_that_op_alone() {
    let client = old_client_of_new_server().await;
    let error = client
        .procedures()
        .retyped(&args!(retyped, "c"))
        .await
        .expect_err("the old argument shape is no longer served");
    match error {
        RpcClientError::Envelope(EnvelopeError::ContractUnsupported { op }) => {
            assert_eq!(op, "procedure.retyped");
        }
        other => panic!("expected ContractUnsupported, got {other:?}"),
    }
    // The refusal is per op: the next call to an unchanged op still works.
    let ping = client.procedures().ping(&args!(ping, "d")).await;
    assert_eq!(ping.expect("other ops are unaffected").echo, "ping:d");
}

#[tokio::test]
async fn a_signed_batch_binds_the_whole_contract_so_any_edit_refuses_it() {
    let client = old_client_of_new_server().await;
    let mut batch = client.batch();
    let _ = client
        .procedures()
        .ping(&args!(ping, "e"))
        .queue(&mut batch);
    match batch.send().await {
        Err(RpcClientError::Envelope(EnvelopeError::ContractUnsupported { op })) => {
            assert_eq!(op, "batch");
        }
        Err(other) => panic!("expected ContractUnsupported, got {other:?}"),
        Ok(_) => panic!("the interim batch binding is the client contract digest"),
    }
}
