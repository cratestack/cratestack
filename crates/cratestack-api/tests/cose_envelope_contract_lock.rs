//! EXT-14 (cratestack#1123), the compatible-contract lock, end to end: a
//! client generated from the schema as it shipped (`contract_lock_old`)
//! against a server generated from the schema after an edit
//! (`contract_lock_new`), over a real listener with the envelope layer in
//! `Required` mode and a COSE-signed client.
//!
//! The edit adds an optional field to an args type and to a reply type
//! (additive, so wire-compatible, but both move the digest of every op that
//! reaches them), edits a policy, and retypes one argument (breaking).
//!
//! - Without a lock the server accepts only its current digests: every op
//!   whose closure moved is refused with the unsigned `426`.
//! - With `contracts = "...lock"`, holding the shipped generation (minus the
//!   breaking op, which `contract prune --op` dropped on purpose), the
//!   compatible ops open under the old digest and their responses are sealed
//!   under it; the decoded input has the new optional field as `None`, and
//!   the old client ignores the reply field it does not know. The breaking
//!   op is still the `426`.

mod cose_client_support;

use cose_client_support::{AUDIENCE, Kind, client_envelope, layer, runtime, serve};
use cratestack::{CratestackContext, CratestackError};
use cratestack_client_rust::{CborCodec, EnvelopeError, RpcClientError};

mod old {
    cratestack::include_client_schema!("tests/fixtures/contract_lock_old.cstack");
}

mod locked {
    cratestack::include_server_schema!(
        "tests/fixtures/contract_lock_new.cstack",
        db = None,
        contracts = "tests/fixtures/contract_lock_new.contracts.lock"
    );
}

mod unlocked {
    cratestack::include_server_schema!("tests/fixtures/contract_lock_new.cstack", db = None);
}

use old::cratestack_schema as shipped;

#[derive(Clone, Default)]
struct Procedures;

/// The same three handlers for the locked and the unlocked server module.
macro_rules! procedures {
    ($module:ident) => {
        impl $module::cratestack_schema::procedures::ProcedureRegistry for Procedures {
            fn ping(
                &self,
                _db: &$module::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $module::cratestack_schema::procedures::ping::Args,
                _authorized: $module::cratestack_schema::procedures::ping::Authorized,
            ) -> impl core::future::Future<Output = Reply<$module::cratestack_schema::PingReply>> + Send
            {
                // `hint` is the field an old client never sends.
                let echo = format!("ping:{}:{:?}", args.args.message, args.args.hint);
                async move {
                    Ok($module::cratestack_schema::PingReply {
                        echo,
                        extra: Some("new field".to_owned()),
                    })
                }
            }

            fn retyped(
                &self,
                _db: &$module::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $module::cratestack_schema::procedures::retyped::Args,
                _authorized: $module::cratestack_schema::procedures::retyped::Authorized,
            ) -> impl core::future::Future<Output = Reply<$module::cratestack_schema::PingReply>> + Send
            {
                let echo = format!("retyped:{}", args.args.message);
                async move {
                    Ok($module::cratestack_schema::PingReply { echo, extra: None })
                }
            }

            fn untouched(
                &self,
                _db: &$module::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $module::cratestack_schema::procedures::untouched::Args,
                _authorized: $module::cratestack_schema::procedures::untouched::Authorized,
            ) -> impl core::future::Future<Output = Reply<$module::cratestack_schema::PingReply>> + Send
            {
                let echo = format!("untouched:{}", args.args.message);
                async move {
                    Ok($module::cratestack_schema::PingReply { echo, extra: None })
                }
            }
        }
    };
}

type Reply<T> = Result<T, CratestackError>;

procedures!(locked);
procedures!(unlocked);

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

type Client = shipped::client::Client<CborCodec>;

macro_rules! old_client_of {
    ($module:ident) => {{
        use $module::cratestack_schema as srv;
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
    }};
}

macro_rules! args {
    ($op:ident, $message:expr) => {
        shipped::procedures::$op::Args {
            args: shipped::PingArgs {
                message: $message.to_owned(),
            },
        }
    };
}

fn accepted(table: &'static [(&str, &[[u8; 32]])], op: &str) -> Vec<[u8; 32]> {
    table
        .iter()
        .find(|(key, _)| *key == op)
        .unwrap_or_else(|| panic!("no row for {op}"))
        .1
        .to_vec()
}

fn shipped_digest(op: &str) -> [u8; 32] {
    shipped::OP_CONTRACTS
        .iter()
        .find(|(key, _)| *key == op)
        .unwrap_or_else(|| panic!("the old client has no {op}"))
        .1
}

fn assert_unsupported<T: std::fmt::Debug>(result: Result<T, RpcClientError>, op: &str) {
    match result {
        Err(RpcClientError::Envelope(EnvelopeError::ContractUnsupported { op: refused })) => {
            assert_eq!(refused, op);
        }
        other => panic!("expected the 426 ContractUnsupported for {op}, got {other:?}"),
    }
}

#[test]
fn the_lock_adds_the_shipped_digest_of_the_compatible_ops_only() {
    let table = locked::cratestack_schema::ACCEPTED_CONTRACTS;
    for op in ["procedure.ping", "procedure.untouched"] {
        let digests = accepted(table, op);
        assert_eq!(digests.len(), 2, "{op}: current, then the locked one");
        assert_ne!(digests[0], shipped_digest(op), "the closure moved");
        assert_eq!(digests[1], shipped_digest(op), "{op}");
    }
    // The pruned, breaking op takes no history, and `batch` never does.
    assert_eq!(accepted(table, "procedure.retyped").len(), 1);
    assert_eq!(accepted(table, "batch").len(), 1);
    // Without the `contracts` argument every row is the current digest alone.
    assert!(
        unlocked::cratestack_schema::ACCEPTED_CONTRACTS
            .iter()
            .all(|(_, digests)| digests.len() == 1)
    );
}

#[tokio::test]
async fn compatible_ops_open_under_the_locked_digest_and_seal_under_it() {
    let client = old_client_of!(locked);
    let ping = client.procedures().ping(&args!(ping, "a")).await;
    // Opened (the client verified the sealed response, so it was sealed
    // under the old digest), the new field decoded as `None`, and the old
    // client ignored the reply field it does not know.
    assert_eq!(ping.expect("locked and compatible").echo, "ping:a:None");
    let untouched = client.procedures().untouched(&args!(untouched, "b")).await;
    assert_eq!(
        untouched.expect("locked and compatible").echo,
        "untouched:b"
    );
}

#[tokio::test]
async fn the_breaking_op_is_still_the_426_while_the_rest_work() {
    let client = old_client_of!(locked);
    assert_unsupported(
        client.procedures().retyped(&args!(retyped, "c")).await,
        "procedure.retyped",
    );
    let ping = client.procedures().ping(&args!(ping, "d")).await;
    assert_eq!(ping.expect("other ops are unaffected").echo, "ping:d:None");
}

#[tokio::test]
async fn without_the_lock_every_op_whose_closure_moved_is_refused() {
    let client = old_client_of!(unlocked);
    for result in [
        client
            .procedures()
            .ping(&args!(ping, "e"))
            .await
            .map(|_| ()),
        client
            .procedures()
            .untouched(&args!(untouched, "f"))
            .await
            .map(|_| ()),
        client
            .procedures()
            .retyped(&args!(retyped, "g"))
            .await
            .map(|_| ()),
    ] {
        assert!(
            matches!(
                result,
                Err(RpcClientError::Envelope(
                    EnvelopeError::ContractUnsupported { .. }
                ))
            ),
            "{result:?}"
        );
    }
}

/// The lock fixture is what `cratestack contract lock` then `prune --op
/// procedure.retyped` writes for the old schema: regenerate it with
/// `CRATESTACK_CONTRACT_WRITE_LOCK=1` (the macro reads it at compile time,
/// so the test binary needs a rebuild afterwards).
#[test]
fn the_checked_in_lock_is_the_old_generation_minus_the_breaking_op() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let source = std::fs::read_to_string(format!(
        "{manifest}/tests/fixtures/contract_lock_old.cstack"
    ))
    .expect("old fixture");
    let schema = cratestack_parser::parse_schema(&source).expect("parses");
    let mut lock = cratestack::ContractLock::new();
    lock.lock_generation(&schema, "2026-10-01", "store 1.0")
        .expect("a real date");
    lock.prune_op("procedure.retyped");
    let path = format!("{manifest}/tests/fixtures/contract_lock_new.contracts.lock");
    if std::env::var("CRATESTACK_CONTRACT_WRITE_LOCK").as_deref() == Ok("1") {
        std::fs::write(&path, lock.to_json()).expect("write lock");
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("lock"),
        lock.to_json()
    );
}
