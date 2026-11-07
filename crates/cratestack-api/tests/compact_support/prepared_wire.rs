//! Observe the exact prepared payload after opening each fresh COSE seal.
#[macro_export]
macro_rules! prepared_sealed_wire_suite {
    ($route:expr, $path:expr, $pattern:expr) => {
        #[tokio::test]
        async fn sealed_prepared_preserves_original_cbor_bytes() {
            use cratestack::axum::{
                Router,
                body::to_bytes,
                extract::Request,
                middleware::{Next, from_fn},
                response::IntoResponse,
                routing::any,
            };
            use std::sync::{Arc, Mutex};
            let captured = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
            let copy = captured.clone();
            let router = Router::new()
                .route(
                    $pattern,
                    any(|| async {
                        ([("content-type", "application/cbor")], vec![0xf6]).into_response()
                    }),
                )
                .layer(from_fn(move |request: Request, _next: Next| {
                    let copy = copy.clone();
                    async move {
                        let body = to_bytes(request.into_body(), 1024).await.unwrap().to_vec();
                        copy.lock().unwrap().push(body);
                        ([("content-type", "application/cbor")], vec![0xf6]).into_response()
                    }
                }));
            let logs = Logs::default();
            let addr = compact_sealed_support::serve(
                router,
                &logs,
                OptIn::Both,
                schema::axum::envelope_layer,
            )
            .await;
            let runtime = sealed_runtime(addr)
                .with_contracts(schema::OP_CONTRACTS)
                .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
                .unwrap();
            let args = client_schema::procedures::echo_order::Args { order: order() };
            let original = runtime
                .prepare_call(
                    "POST",
                    cratestack_client_rust::RouteRef::new($route, &[]),
                    $path,
                    &args,
                    &[],
                    &[("Idempotency-Key", "k")],
                )
                .unwrap();
            let json: serde_json::Value =
                serde_json::from_slice(&original.to_bytes().unwrap()).unwrap();
            let bodies = [
                vec![0x18, 0],
                vec![0x5f, 0x41, 0, 0x42, 1, 2, 0xff],
                vec![0xf9, 0x7e, 0],
            ];
            for body in &bodies {
                let mut changed = json.clone();
                changed["body"] = serde_json::json!(body);
                let call = cratestack_client_rust::PreparedCall::from_bytes(
                    &serde_json::to_vec(&changed).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    runtime.execute_prepared(&call).await.unwrap().status_code,
                    200
                );
            }
            assert_eq!(*captured.lock().unwrap(), bodies);
        }
    };
}

#[macro_export]
macro_rules! sealed_authorizer_pin_suite {
    ($route:expr, $path:expr) => {
        struct SealedAuthorizerOnlyKey;
        #[async_trait::async_trait]
        impl cratestack_client_rust::RequestAuthorizer for SealedAuthorizerOnlyKey {
            async fn authorize(
                &self,
                _: &cratestack_client_rust::AuthorizationRequest,
            ) -> Result<Vec<(String, String)>, cratestack_client_rust::ClientError> {
                Ok(vec![("Idempotency-Key".into(), "authorizer-key".into())])
            }
        }
        #[tokio::test]
        async fn sealed_authorizer_only_key_is_refused_before_network() {
            use std::sync::Arc;
            let (addr, logs) = serve_sealed(OptIn::Both).await;
            let runtime = sealed_runtime(addr)
                .with_contracts(schema::OP_CONTRACTS)
                .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
                .unwrap()
                .with_request_authorizer(Arc::new(SealedAuthorizerOnlyKey));
            let scoped = runtime.at(cratestack_client_rust::RouteRef::new($route, &[]));
            let args = client_schema::procedures::echo_order::Args { order: order() };
            let rejected = scoped
                .post::<_, client_schema::Order>($path, &args, &[])
                .await;
            assert!(matches!(
                rejected,
                Err(cratestack_client_rust::ClientError::BadInput(_))
            ));
            assert!(logs.wire.lock().unwrap().is_empty());
            for _ in 0..2 {
                let _: client_schema::Order = scoped
                    .post($path, &args, &[("Idempotency-Key", "explicit")])
                    .await
                    .unwrap();
            }
            let inner = logs.inner.lock().unwrap();
            assert_eq!(inner.len(), 2);
            assert_eq!(inner[0].0, "application/cbor");
            assert_eq!(inner[1].0, inner[0].0);
        }
    };
}
