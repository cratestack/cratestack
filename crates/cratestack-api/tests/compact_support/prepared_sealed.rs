//! Sealed prepared requests use a fresh envelope on each replay.

#[macro_export]
macro_rules! prepared_sealed_suite {
    ($router:path, $route:expr, $path:expr) => {
        async fn sealed_replay(warm_first: bool) {
            use cratestack::axum::{
                extract::Request,
                middleware::{Next, from_fn},
            };
            use std::sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            };
            let executions = Arc::new(AtomicUsize::new(0));
            let count = executions.clone();
            let router = $router(
                schema::Cratestack::builder().build(),
                Procedures,
                (),
                CodecSet::new(CborCodec, JsonCodec),
                Anonymous,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
            .layer(from_fn(move |request: Request, next: Next| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    next.run(request).await
                }
            }))
            .layer(
                cratestack_axum::idempotency::IdempotencyLayer::new(
                    Arc::new(compact_support::idempotency::InMemoryIdempotencyStore::default()),
                    std::time::Duration::from_secs(3600),
                )
                .with_principal_fingerprint(|_| "sealed-test-principal".to_owned()),
            );
            let logs = Logs::default();
            let addr = compact_sealed_support::serve(
                router,
                &logs,
                OptIn::Both,
                schema::axum::envelope_layer,
            )
            .await;
            let make = || {
                sealed_runtime(addr)
                    .with_contracts(schema::OP_CONTRACTS)
                    .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
                    .unwrap()
            };
            let runtime = make();
            let route = || cratestack_client_rust::RouteRef::new($route, &[]);
            let args = client_schema::procedures::echo_order::Args { order: order() };
            let scoped = runtime.at(route());
            if warm_first {
                let _: client_schema::Order = scoped.post($path, &args, &[]).await.unwrap();
            }
            let call = runtime
                .prepare_call(
                    "POST",
                    route(),
                    $path,
                    &args,
                    &[],
                    &[("Idempotency-Key", "saved")],
                )
                .unwrap();
            let encoded = call.to_bytes().unwrap();
            assert_eq!(
                runtime.execute_prepared(&call).await.unwrap().status_code,
                200
            );
            for n in 0..1024 {
                let key = format!("pressure-{n}");
                let _: client_schema::Order = scoped
                    .post($path, &args, &[("Idempotency-Key", &key)])
                    .await
                    .unwrap();
            }
            let before = executions.load(Ordering::SeqCst);
            let restored = cratestack_client_rust::PreparedCall::from_bytes(&encoded).unwrap();
            assert_eq!(
                make()
                    .execute_prepared(&restored)
                    .await
                    .unwrap()
                    .status_code,
                200
            );
            assert_eq!(executions.load(Ordering::SeqCst), before);
            let seen = logs.inner.lock().unwrap();
            let first = usize::from(warm_first);
            assert_eq!(seen[first].0, seen.last().unwrap().0);
            assert_eq!(
                seen[first].0 == cratestack::compact::COMPACT_CBOR,
                warm_first
            );
        }
        #[tokio::test]
        async fn sealed_prepared_named_replays() {
            sealed_replay(false).await;
        }
        #[tokio::test]
        async fn sealed_prepared_compact_replays() {
            sealed_replay(true).await;
        }
    };
}
