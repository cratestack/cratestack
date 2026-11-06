//! Exercise authorizer output through the actual prepared transport.
#[macro_export]
macro_rules! prepared_auth_suite {
    ($router:path, $route:expr, $path:expr) => {
        struct IdentityOverridingAuthorizer;
        #[async_trait::async_trait]
        impl cratestack_client_rust::RequestAuthorizer for IdentityOverridingAuthorizer {
            async fn authorize(
                &self,
                _: &cratestack_client_rust::AuthorizationRequest,
            ) -> Result<Vec<(String, String)>, cratestack_client_rust::ClientError> {
                Ok(vec![
                    ("Idempotency-Key".into(), "replacement".into()),
                    ("If-Match".into(), "replacement".into()),
                    ("Authorization".into(), "fresh".into()),
                ])
            }
        }

        #[tokio::test]
        async fn authorizer_cannot_change_frozen_identity_headers() {
            use cratestack::axum::{
                extract::Request,
                middleware::{Next, from_fn},
            };
            let seen = Arc::new(Mutex::new(Vec::new()));
            let captured = seen.clone();
            let router = $router(
                schema::Cratestack::builder().build(),
                Procedures,
                (),
                CodecSet::new(CborCodec, JsonCodec),
                Anonymous,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
            .layer(from_fn(move |request: Request, next: Next| {
                let captured = captured.clone();
                async move {
                    let headers = request.headers();
                    let values = ["idempotency-key", "if-match", "authorization"].map(|name| {
                        headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned)
                    });
                    captured.lock().unwrap().push(values);
                    next.run(request).await
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { cratestack::axum::serve(listener, router).await.unwrap() });
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap()
            .with_request_authorizer(Arc::new(IdentityOverridingAuthorizer));
            for headers in [
                &[("Idempotency-Key", "original")][..],
                &[
                    ("Idempotency-Key", "original"),
                    ("If-Match", "original-etag"),
                ][..],
            ] {
                let call = runtime
                    .prepare_call(
                        "POST",
                        cratestack_client_rust::RouteRef::new($route, &[]),
                        $path,
                        &echo(order()),
                        &[],
                        headers,
                    )
                    .unwrap();
                assert_eq!(
                    runtime.execute_prepared(&call).await.unwrap().status_code,
                    200
                );
            }
            let seen = seen.lock().unwrap();
            assert_eq!(
                seen[0],
                [Some("original".into()), None, Some("fresh".into())]
            );
            assert_eq!(
                seen[1],
                [
                    Some("original".into()),
                    Some("original-etag".into()),
                    Some("fresh".into())
                ]
            );
        }
        #[tokio::test]
        async fn ordinary_authorizer_key_must_be_explicit_for_compact_pinning() {
            let (addr, log, executions) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap()
            .with_request_authorizer(Arc::new(IdentityOverridingAuthorizer));
            let scoped = runtime.at(cratestack_client_rust::RouteRef::new($route, &[]));
            let rejected = scoped
                .post::<_, client_schema::Order>($path, &echo(order()), &[])
                .await;
            assert!(matches!(
                rejected,
                Err(cratestack_client_rust::ClientError::BadInput(_))
            ));
            assert_eq!(executions.load(Ordering::SeqCst), 0);
            assert!(log.lock().unwrap().is_empty());
            for _ in 0..2 {
                let _: client_schema::Order = scoped
                    .post($path, &echo(order()), &[("Idempotency-Key", "original")])
                    .await
                    .unwrap();
            }
            assert_eq!(executions.load(Ordering::SeqCst), 1);
            let seen = log.lock().unwrap();
            assert_eq!(seen[0].0, seen[1].0);
        }
    };
}
