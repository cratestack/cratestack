//! Duplicate identity headers fail before a typed input is encoded.
#[macro_export]
macro_rules! compact_duplicate_encoding_suite {
    ($route:expr, $path:expr) => {
        #[tokio::test]
        async fn duplicate_key_is_rejected_before_serialization() {
            struct Counted<'a>(&'a AtomicUsize);
            impl serde::Serialize for Counted<'_> {
                fn serialize<S: serde::Serializer>(
                    &self,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    self.0.fetch_add(1, Ordering::SeqCst);
                    serializer.serialize_u8(1)
                }
            }
            let (addr, log, _) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap();
            let scoped = runtime.at(cratestack_client_rust::RouteRef::new($route, &[]));
            let encoded = AtomicUsize::new(0);
            let result = scoped
                .post::<_, client_schema::Order>(
                    $path,
                    &Counted(&encoded),
                    &[("Idempotency-Key", "a"), ("idempotency-key", "b")],
                )
                .await;
            assert!(matches!(
                result,
                Err(cratestack_client_rust::ClientError::BadInput(_))
            ));
            assert_eq!(encoded.load(Ordering::SeqCst), 0);
            assert!(log.lock().unwrap().is_empty());
        }
    };
}
