//! Compact wire keys across a contract lock: a server built with
//! `contracts = "...lock"` keeps the key numbering of every contract it
//! still accepts, so a client built from the older schema is served with its
//! own keys, while the current contract uses the new ones. Without the lock
//! the old selector names no contract, and a compact request is a `415`.
//! Fixtures are those of `cose_envelope_contract_lock.rs`.

mod compact_support;

use compact_support::{Anonymous, call, selector};
use cratestack::compact::{COMPACT_CBOR, Direction, KeyTables};
use cratestack::{CodecSet, CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_codec_json::JsonCodec;

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

#[derive(Clone, Default)]
struct Procedures;

macro_rules! procedures {
    ($m:ident) => {
        impl $m::cratestack_schema::procedures::ProcedureRegistry for Procedures {
            fn ping(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $m::cratestack_schema::procedures::ping::Args,
                _authorized: $m::cratestack_schema::procedures::ping::Authorized,
            ) -> impl core::future::Future<
                Output = Result<$m::cratestack_schema::PingReply, CratestackError>,
            > + Send {
                let echo = format!("ping:{}:{:?}", args.args.message, args.args.hint);
                async move {
                    Ok($m::cratestack_schema::PingReply {
                        echo,
                        extra: Some("new field".to_owned()),
                    })
                }
            }

            fn retyped(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                _args: $m::cratestack_schema::procedures::retyped::Args,
                _authorized: $m::cratestack_schema::procedures::retyped::Authorized,
            ) -> impl core::future::Future<
                Output = Result<$m::cratestack_schema::PingReply, CratestackError>,
            > + Send {
                async move {
                    Ok($m::cratestack_schema::PingReply {
                        echo: String::new(),
                        extra: None,
                    })
                }
            }

            fn untouched(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                _args: $m::cratestack_schema::procedures::untouched::Args,
                _authorized: $m::cratestack_schema::procedures::untouched::Authorized,
            ) -> impl core::future::Future<
                Output = Result<$m::cratestack_schema::PingReply, CratestackError>,
            > + Send {
                async move {
                    Ok($m::cratestack_schema::PingReply {
                        echo: String::new(),
                        extra: None,
                    })
                }
            }
        }
    };
}

procedures!(locked);
procedures!(unlocked);

macro_rules! router {
    ($m:ident) => {
        $m::cratestack_schema::axum::rpc_router(
            $m::cratestack_schema::Cratestack::builder().build(),
            Procedures,
            (),
            CodecSet::new(CborCodec, JsonCodec),
            Anonymous,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    };
}

const PING: &str = "/rpc/procedure.ping";
const CBOR: &str = "application/cbor";

/// The tables of the schema as the shipped client was generated from it.
fn shipped() -> KeyTables {
    let source = std::fs::read_to_string(format!(
        "{}/tests/fixtures/contract_lock_old.cstack",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("old fixture");
    KeyTables::from_schema(&cratestack_parser::parse_schema(&source).expect("parses"))
}

fn ping_args() -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "message": "hi" } }))
        .expect("encodes")
}

#[tokio::test]
async fn an_older_contract_is_served_with_its_own_numbering() {
    let old = shipped();
    let old_keys = old
        .ops
        .iter()
        .find(|keys| keys.op == "procedure.ping")
        .expect("ping");
    let old_selector = selector(&old_keys.digest);
    let current_digest = locked::cratestack_schema::OP_CONTRACTS
        .iter()
        .find(|(key, _)| *key == "procedure.ping")
        .expect("ping")
        .1;
    assert_ne!(
        old_keys.digest, current_digest,
        "the edit moved ping's digest"
    );
    let router = router!(locked);

    // The old client's request: `message` is key 0 of its `PingArgs`.
    let request = cbor_to_compact(&old, old.node(old_keys, Direction::Request), &ping_args())
        .expect("compacts");
    let headers = [
        ("content-type", COMPACT_CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &old_selector),
    ];
    let answer = call(&router, "POST", PING, &headers, request.clone()).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("content-type"), Some(COMPACT_CBOR));

    // The reply has the old client's numbering and lacks the field it does
    // not know (`extra`): `{ "echo": .. }` is `{ 0: .. }`.
    let reply = CborCodec
        .encode(&serde_json::json!({ "echo": "ping:hi:None" }))
        .expect("encodes");
    let expected =
        cbor_to_compact(&old, old.node(old_keys, Direction::Response), &reply).expect("compacts");
    assert_eq!(answer.body, expected);

    // The current selector picks the current numbering, which keeps `extra`.
    let current = {
        let tables = KeyTables::from_bytes(locked::cratestack_schema::KEY_TABLES).expect("tables");
        let keys = tables
            .find_digest("procedure.ping", &current_digest)
            .expect("current")
            .clone();
        (tables, keys)
    };
    let new_selector = selector(&current_digest);
    let named = call(
        &router,
        "POST",
        PING,
        &[
            ("content-type", CBOR),
            ("accept", COMPACT_CBOR),
            ("cratestack-contract", &new_selector),
        ],
        ping_args(),
    )
    .await;
    assert_eq!(named.header("content-type"), Some(COMPACT_CBOR));
    let full = CborCodec
        .encode(&serde_json::json!({ "echo": "ping:hi:None", "extra": "new field" }))
        .expect("encodes");
    let in_current = cbor_to_compact(
        &current.0,
        current.0.node(&current.1, Direction::Response),
        &full,
    )
    .expect("compacts");
    assert_eq!(
        named.body.len(),
        in_current.len(),
        "the current numbering keeps `extra`"
    );
    assert_ne!(named.body, answer.body);
}

#[tokio::test]
async fn without_the_lock_the_old_selector_is_unknown() {
    let old = shipped();
    let keys = old
        .ops
        .iter()
        .find(|keys| keys.op == "procedure.ping")
        .expect("ping");
    let selector = selector(&keys.digest);
    let router = router!(unlocked);
    let body =
        cbor_to_compact(&old, old.node(keys, Direction::Request), &ping_args()).expect("compacts");

    let sent = [
        ("content-type", COMPACT_CBOR),
        ("accept", CBOR),
        ("cratestack-contract", &selector),
    ];
    assert_eq!(call(&router, "POST", PING, &sent, body).await.status, 415);

    // And a compact Accept with that selector falls back to names.
    let asked = [
        ("content-type", CBOR),
        ("accept", COMPACT_CBOR),
        ("cratestack-contract", &selector),
    ];
    let answer = call(&router, "POST", PING, &asked, ping_args()).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("content-type"), Some(CBOR));
}
