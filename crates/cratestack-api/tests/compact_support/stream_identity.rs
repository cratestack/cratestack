//! Compact streaming calls enforce the same identity-header rules.
#[macro_export]
macro_rules! stream_identity_suite {
    ($route:expr, $path:expr) => {
        #[tokio::test]
        async fn compact_stream_rejects_ambiguous_or_authorizer_only_keys() {
            let (addr, log, executions) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap()
            .with_request_authorizer(Arc::new(IdentityOverridingAuthorizer));
            let scoped = runtime.at(cratestack_client_rust::RouteRef::new($route, &[]));
            let args = client_schema::procedures::ticks::Args { count: 2 };
            let no_key = scoped
                .post_list_streamed::<_, client_schema::Item>($path, &args, &[])
                .await;
            assert!(matches!(
                no_key,
                Err(cratestack_client_rust::ClientError::BadInput(_))
            ));
            let duplicate = scoped
                .post_list_streamed::<_, client_schema::Item>(
                    $path,
                    &args,
                    &[("Idempotency-Key", "first"), ("idempotency-key", "second")],
                )
                .await;
            assert!(matches!(
                duplicate,
                Err(cratestack_client_rust::ClientError::BadInput(_))
            ));
            assert!(log.lock().unwrap().is_empty());
            assert_eq!(executions.load(Ordering::SeqCst), 0);
            let mut stream = scoped
                .post_list_streamed::<_, client_schema::Item>(
                    $path,
                    &args,
                    &[("Idempotency-Key", "explicit")],
                )
                .await
                .unwrap();
            let mut count = 0;
            while let Some(item) = stream.recv().await {
                item.unwrap();
                count += 1;
            }
            assert_eq!(count, 2);
            assert_eq!(executions.load(Ordering::SeqCst), 1);
        }
    };
}
