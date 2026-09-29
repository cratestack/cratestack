//! The generated Rust client seals and opens model calls (cratestack#1007),
//! over REST and RPC, against the real generated server, the real envelope
//! layer and a real Postgres: create, get, a list with a query, an update
//! that carries `If-Match`, and a delete. The procedure half, and every
//! failure path, is in `cratestack-api`'s `cose_client_*` tests; this file
//! is the part that needs models (path parameters, a bound query, `@version`).

#[path = "../../cratestack-api/tests/cose_client_support/mod.rs"]
mod cose_client_support;
mod support;

use cose_client_support::{AUDIENCE, KINDS, client_envelope, layer, runtime, serve};
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_client_rust::{CborCodec, ClientError};

use support::pg;

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

async fn reset(pool: &cratestack::sqlx::PgPool, versioned: bool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, sealed_ledgers, notes")
        .execute(pool)
        .await
        .expect("drop tables");
    query("CREATE TABLE notes (id TEXT PRIMARY KEY, body TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create notes");
    // Two literals rather than a formatted string: sqlx audits dynamic SQL.
    if versioned {
        query(
            "CREATE TABLE sealed_ledgers (id BIGINT PRIMARY KEY, label TEXT NOT NULL, \
             balance BIGINT NOT NULL, version BIGINT NOT NULL DEFAULT 0)",
        )
    } else {
        query(
            "CREATE TABLE sealed_ledgers (id BIGINT PRIMARY KEY, label TEXT NOT NULL, \
             balance BIGINT NOT NULL)",
        )
    }
    .execute(pool)
    .await
    .expect("create sealed_ledgers");
}

mod rest {
    use super::*;

    pub mod server {
        cratestack::include_server_schema!(
            "tests/fixtures/cose_client_models.cstack",
            db = Postgres
        );
    }
    pub mod client {
        cratestack::include_client_schema!("tests/fixtures/cose_client_models.cstack");
    }
    use server::cratestack_schema as srv;

    #[derive(Clone)]
    struct NoProcedures;
    impl srv::procedures::ProcedureRegistry for NoProcedures {}

    #[tokio::test]
    async fn create_get_list_update_and_delete_round_trip_signed() {
        let _guard = pg::serial_guard().await;
        let Some(test_pg) = pg::connect_or_skip().await else {
            return;
        };
        for kind in KINDS {
            reset(&test_pg.pool, true).await;
            let router = srv::axum::router(
                srv::Cratestack::builder(test_pg.pool.clone()).build(),
                NoProcedures,
                (),
                cratestack_codec_cbor::CborCodec,
                PassThroughAuth,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
            .layer(layer(kind, |envelope, policy, audience| {
                srv::axum::envelope_layer(envelope, policy, audience)
            }));
            let addr = serve(router).await;
            let client = client::cratestack_schema::client::Client::<CborCodec>::new(runtime(
                addr,
                client_envelope(kind, AUDIENCE),
            ));
            let ledgers = client.sealed_ledgers();

            let created = ledgers
                .create(
                    &client::cratestack_schema::CreateSealedLedgerInput {
                        id: 1,
                        label: "gl-1".to_owned(),
                        balance: 0,
                    },
                    &[],
                )
                .await
                .unwrap_or_else(|error| panic!("{kind:?} create: {error}"));
            assert_eq!(created.label, "gl-1");

            let got = ledgers
                .get_with_response(&1, &[])
                .await
                .unwrap_or_else(|error| panic!("{kind:?} get: {error}"));
            assert_eq!(got.value.label, "gl-1");
            let etag = got.header("etag").expect("an ETag").to_owned();

            // The query is bound: `limit=5` is part of what the server verifies.
            let listed = ledgers
                .list(&[("limit", "5")], &[])
                .await
                .unwrap_or_else(|error| panic!("{kind:?} list: {error}"));
            assert_eq!(listed.len(), 1, "{kind:?}");

            let patch = client::cratestack_schema::UpdateSealedLedgerInput {
                label: None,
                balance: Some(7),
            };
            let stale = ledgers
                .update_with_response(&1, &patch, &[("If-Match", "\"99\"")])
                .await;
            assert!(
                matches!(&stale, Err(ClientError::Remote { status, .. }) if status.as_u16() == 412),
                "{kind:?}: a sealed 412 is the usual Remote error, got {stale:?}"
            );
            let updated = ledgers
                .update_with_response(&1, &patch, &[("If-Match", etag.as_str())])
                .await
                .unwrap_or_else(|error| panic!("{kind:?} update: {error}"));
            assert_eq!(updated.value.balance, 7);
            let etag = updated.header("etag").expect("a new ETag").to_owned();

            ledgers
                .delete_with_response(&1, &[("If-Match", etag.as_str())])
                .await
                .unwrap_or_else(|error| panic!("{kind:?} delete: {error}"));
        }
    }

    // The id is percent-encoded on the wire and bound raw: `50%off` used to
    // be a permanent 401, and `a/b` a different route.
    #[tokio::test]
    async fn a_string_id_with_reserved_characters_round_trips_signed() {
        let _guard = pg::serial_guard().await;
        let Some(test_pg) = pg::connect_or_skip().await else {
            return;
        };
        for kind in KINDS {
            reset(&test_pg.pool, true).await;
            let router = srv::axum::router(
                srv::Cratestack::builder(test_pg.pool.clone()).build(),
                NoProcedures,
                (),
                cratestack_codec_cbor::CborCodec,
                PassThroughAuth,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
            .layer(layer(kind, |envelope, policy, audience| {
                srv::axum::envelope_layer(envelope, policy, audience)
            }));
            let addr = serve(router).await;
            let client = client::cratestack_schema::client::Client::<CborCodec>::new(runtime(
                addr,
                client_envelope(kind, AUDIENCE),
            ));
            for id in ["50%off", "a/b c", "plain-1", "q?x=1#f"] {
                client
                    .notes()
                    .create(
                        &client::cratestack_schema::CreateNoteInput {
                            id: id.to_owned(),
                            body: format!("body of {id}"),
                        },
                        &[],
                    )
                    .await
                    .unwrap_or_else(|error| panic!("{kind:?} create {id}: {error}"));
                let got = client
                    .notes()
                    .get(&id.to_owned(), &[])
                    .await
                    .unwrap_or_else(|error| panic!("{kind:?} get {id}: {error}"));
                assert_eq!(got.body, format!("body of {id}"), "{kind:?} {id}");
            }
        }
    }
}

mod rpc {
    use super::*;

    pub mod server {
        cratestack::include_server_schema!(
            "tests/fixtures/cose_client_models_rpc.cstack",
            db = Postgres
        );
    }
    pub mod client {
        cratestack::include_client_schema!("tests/fixtures/cose_client_models_rpc.cstack");
    }
    use server::cratestack_schema as srv;

    #[derive(Clone)]
    struct NoProcedures;
    impl srv::procedures::ProcedureRegistry for NoProcedures {}

    #[tokio::test]
    async fn create_get_list_update_and_delete_round_trip_signed_including_a_batch() {
        let _guard = pg::serial_guard().await;
        let Some(test_pg) = pg::connect_or_skip().await else {
            return;
        };
        for kind in KINDS {
            reset(&test_pg.pool, false).await;
            let router = srv::axum::rpc_router(
                srv::Cratestack::builder(test_pg.pool.clone()).build(),
                NoProcedures,
                (),
                cratestack_codec_cbor::CborCodec,
                PassThroughAuth,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
            .layer(layer(kind, |envelope, policy, audience| {
                srv::axum::envelope_layer(envelope, policy, audience)
            }));
            let addr = serve(router).await;
            let client = client::cratestack_schema::client::Client::<CborCodec>::new(runtime(
                addr,
                client_envelope(kind, AUDIENCE),
            ));
            let ledgers = client.sealed_ledgers();

            let create = |id: i64| client::cratestack_schema::CreateSealedLedgerInput {
                id,
                label: format!("gl-{id}"),
                balance: 0,
            };
            ledgers
                .create(&create(1))
                .await
                .unwrap_or_else(|error| panic!("{kind:?} create: {error}"));
            let got = ledgers
                .get(&1)
                .await
                .unwrap_or_else(|error| panic!("{kind:?} get: {error}"));
            assert_eq!(got.label, "gl-1");

            let patch = client::cratestack_schema::UpdateSealedLedgerInput {
                label: None,
                balance: Some(7),
            };
            let updated = ledgers
                .update(&1, &patch)
                .await
                .unwrap_or_else(|error| panic!("{kind:?} update: {error}"));
            assert_eq!(updated.balance, 7);

            // One signed message carries two ops.
            let mut batch = client.batch();
            let second = ledgers.create(&create(2)).queue(&mut batch);
            let listed = ledgers.list(&Default::default()).queue(&mut batch);
            let mut results = batch
                .send()
                .await
                .unwrap_or_else(|error| panic!("{kind:?} batch: {error}"));
            assert_eq!(results.take(second).expect("create").id, 2);
            assert_eq!(results.take(listed).expect("list").len(), 2, "{kind:?}");

            ledgers
                .delete(&1)
                .await
                .unwrap_or_else(|error| panic!("{kind:?} delete: {error}"));
        }
    }
}
