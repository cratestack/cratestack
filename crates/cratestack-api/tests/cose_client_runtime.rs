//! The FFI runtime seals too (cratestack#1007): `RuntimeHandle::with_envelope`
//! completes a signed round trip against the real generated RPC server, and
//! a config that names an envelope no one supplied, or the reverse, is a
//! `BadInput` that says what to do. The handle is what Flutter and the other
//! bridges hold; a signed call needs no Dart code to know about COSE.

mod cose_client_support;
#[path = "cose_client_support/rpc_app.rs"]
mod rpc_app;

use cose_client_support::{AUDIENCE, KINDS, Kind, alg_of, client_envelope};
use cratestack_client_rust::{
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
    RuntimeErrorWire, RuntimeHandle, RuntimeHeader, RuntimeRequestWire, RuntimeStateStoreConfig,
    RuntimeTransportConfig,
};
use rpc_app::server::cratestack_schema::OP_CONTRACTS;

fn config(addr: std::net::SocketAddr, envelope: RuntimeEnvelopeConfig) -> RuntimeConfigWire {
    RuntimeConfigWire {
        base_url: format!("http://{addr}"),
        state_store: RuntimeStateStoreConfig::InMemory,
        transport: RuntimeTransportConfig {
            codec: RuntimeCodecConfig::Cbor,
            envelope,
        },
    }
}

fn named(kind: Kind) -> RuntimeEnvelopeConfig {
    match kind {
        Kind::Mac0 => RuntimeEnvelopeConfig::CoseMac0,
        _ => RuntimeEnvelopeConfig::CoseSign1,
    }
}

fn ping(path: &str) -> RuntimeRequestWire {
    RuntimeRequestWire {
        method: "POST".to_owned(),
        path: path.to_owned(),
        canonical_query: None,
        headers: Vec::<RuntimeHeader>::new(),
        body: br#"{"args":{"message":"hi"}}"#.to_vec(),
    }
}

/// `RuntimeHandle` owns a runtime and blocks on it, so it lives, runs and
/// drops on a blocking thread, never on the test's own runtime.
async fn on_blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(work).await.expect("join")
}

#[tokio::test]
async fn a_handle_with_an_envelope_completes_a_signed_round_trip() {
    for kind in KINDS {
        let addr = rpc_app::server(kind).await;
        let response = on_blocking(move || {
            let handle = RuntimeHandle::with_envelope(
                config(addr, named(kind)),
                client_envelope(kind, AUDIENCE),
                OP_CONTRACTS,
            )
            .expect("handle");
            handle.execute(ping("/rpc/procedure.ping"))
        })
        .await
        .unwrap_or_else(|error| panic!("{kind:?}: {error:?}"));
        assert_eq!(response.status_code, 200, "{kind:?}");
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("JSON");
        assert_eq!(
            body["echo"],
            format!("v2:hi|signer=Some({})", alg_of(kind)),
            "{kind:?}"
        );
    }
}

fn bad_input(error: &RuntimeErrorWire) -> &str {
    assert_eq!(error.code, RuntimeErrorCode::BadInput, "{error:?}");
    &error.message
}

#[tokio::test]
async fn a_config_that_names_an_envelope_without_one_is_a_pointed_bad_input() {
    let addr = rpc_app::server(Kind::Ed25519).await;
    for named in [
        RuntimeEnvelopeConfig::CoseSign1,
        RuntimeEnvelopeConfig::CoseMac0,
    ] {
        let error = on_blocking(move || RuntimeHandle::new(config(addr, named)).err())
            .await
            .expect("refused");
        assert!(bad_input(&error).contains("with_envelope"), "{error:?}");
    }
}

#[tokio::test]
async fn an_envelope_the_config_does_not_name_is_refused() {
    let addr = rpc_app::server(Kind::Ed25519).await;
    let refused = |envelope: RuntimeEnvelopeConfig, kind: Kind| {
        on_blocking(move || {
            RuntimeHandle::with_envelope(
                config(addr, envelope),
                client_envelope(kind, AUDIENCE),
                OP_CONTRACTS,
            )
            .err()
        })
    };
    let none = refused(RuntimeEnvelopeConfig::None, Kind::Ed25519)
        .await
        .expect("None");
    assert!(bad_input(&none).contains("names None"), "{none:?}");
    let wrong = refused(RuntimeEnvelopeConfig::CoseMac0, Kind::Ed25519)
        .await
        .expect("mode");
    assert!(bad_input(&wrong).contains("CoseSign1"), "{wrong:?}");
}

#[tokio::test]
async fn a_raw_rest_path_has_no_route_to_bind_and_a_stream_is_refused() {
    let addr = rpc_app::server(Kind::Ed25519).await;
    let (rest, stream) = on_blocking(move || {
        let handle = RuntimeHandle::with_envelope(
            config(addr, RuntimeEnvelopeConfig::CoseSign1),
            client_envelope(Kind::Ed25519, AUDIENCE),
            OP_CONTRACTS,
        )
        .expect("handle");
        let rest = handle.execute(ping("/v2/$procs/ping")).err();
        let stream = handle
            .execute_streamed(ping("/rpc/procedure.ping"), |_| true)
            .err();
        (rest, stream)
    })
    .await;
    assert!(bad_input(&rest.expect("refused")).contains("RPC"));
    let stream = stream.expect("refused");
    assert_eq!(
        stream.remote_code.as_deref(),
        Some("envelope_streams_unsupported")
    );
}
