//! Compact wire keys under the COSE envelope, REST (ADR 0006,
//! `docs/design/compact-wire-keys.md` §5.1): the generated Rust client with
//! `with_envelope` and `with_compact_keys` against the generated server
//! behind an `EnvelopeLayer`. The RPC twin is `compact_client_sealed_rpc.rs`;
//! the test bodies are shared (`sealed_compact_suite!`).

mod compact_sealed_support;
mod compact_support;
mod cose_client_support;

use compact_sealed_support::{Logs, OptIn, sealed_runtime};
use compact_support::Anonymous;
use cratestack::{CodecSet, CratestackContext, CratestackError};
use cratestack_client_rust::{ClientError, EnvelopeError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

mod srv {
    cratestack::include_server_schema!("tests/fixtures/compact_rest.cstack", db = None);
}
mod cli {
    cratestack::include_client_schema!("tests/fixtures/compact_rest.cstack");
}

use cli::cratestack_schema as client_schema;
use srv::cratestack_schema as schema;

compact_procedures!(srv);

async fn serve_sealed(opt: OptIn) -> (std::net::SocketAddr, Logs) {
    let logs = Logs::default();
    let router = schema::axum::router(
        schema::Cratestack::builder().build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let addr =
        compact_sealed_support::serve(router, &logs, opt, schema::axum::envelope_layer).await;
    (addr, logs)
}

fn client(addr: std::net::SocketAddr, compact: bool) -> client_schema::client::Client {
    let runtime = sealed_runtime(addr);
    if compact {
        client_schema::client::Client::with_compact_keys(runtime)
    } else {
        client_schema::client::Client::new(runtime)
    }
}

fn order() -> client_schema::Order {
    client_schema::Order {
        id: 7,
        name: "ada".to_owned(),
        note: None,
        address: client_schema::Address {
            street: "main".to_owned(),
            city: "oslo".to_owned(),
        },
        tags: vec!["a".to_owned(), "b".to_owned()],
    }
}

async fn try_echo(
    client: &client_schema::client::Client,
) -> Result<client_schema::Order, ClientError> {
    let args = client_schema::procedures::echo_order::Args { order: order() };
    client.procedures().echo_order(&args, &[]).await
}

fn is_unsigned_415(result: &Result<client_schema::Order, ClientError>) -> bool {
    matches!(
        result,
        Err(ClientError::Envelope(EnvelopeError::Unsigned {
            status: 415
        }))
    )
}

async fn echo(client: &client_schema::client::Client) {
    let back = try_echo(client).await.expect("a sealed echo");
    assert_eq!(back.id, 7);
    assert_eq!(back.address.city, "oslo");
    assert_eq!(back.tags, ["a", "b"]);
    assert_eq!(back.note, None);
}

async fn stream_refused(client: &client_schema::client::Client) -> bool {
    let args = client_schema::procedures::ticks::Args { count: 2 };
    matches!(
        client
            .runtime()
            .post_list_streamed::<_, client_schema::Item>("/$procs/ticks", &args, &[])
            .await,
        Err(ClientError::Envelope(EnvelopeError::StreamsUnsupported))
    )
}

sealed_compact_suite!();

prepared_sealed_suite!(
    schema::axum::router,
    "/$procs/echoOrder",
    "/$procs/echoOrder"
);
