//! Persisted calls survive negotiation state loss and idempotency-cache pressure.

#[macro_export]
macro_rules! prepared_replay_suite {
    ($route:expr, $path:expr) => {
        async fn prepared_replay(warm_first: bool) {
            let (addr, log, executions) = spawn().await;
            let make = || {
                CratestackClient::cbor(ClientConfig::new(
                    reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
                ))
                .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
                .unwrap()
            };
            let runtime = make();
            let route = || cratestack_client_rust::RouteRef::new($route, &[]);
            let scoped = runtime.at(route());
            if warm_first {
                let _: client_schema::Order =
                    scoped.post($path, &echo(order()), &[]).await.unwrap();
            }
            let snapshot = runtime
                .prepare_call(
                    "POST",
                    route(),
                    $path,
                    &echo(order()),
                    &[],
                    &[("Idempotency-Key", "saved")],
                )
                .unwrap();
            let encoded = snapshot.to_bytes().unwrap();
            let original = runtime.execute_prepared(&snapshot).await.unwrap();
            assert_eq!(original.status_code, 200);
            for n in 0..1024 {
                let key = format!("pressure-{n}");
                let _: client_schema::Order = scoped
                    .post($path, &echo(order()), &[("Idempotency-Key", &key)])
                    .await
                    .unwrap();
            }
            let before = executions.load(Ordering::SeqCst);
            let restored = cratestack_client_rust::PreparedCall::from_bytes(&encoded).unwrap();
            let retry = make().execute_prepared(&restored).await.unwrap();
            assert_eq!(retry.status_code, 200);
            assert_eq!(executions.load(Ordering::SeqCst), before);
            let seen = log.lock().unwrap();
            let first = usize::from(warm_first);
            assert_eq!(seen[first].0, seen.last().unwrap().0);
            assert_eq!(seen[first].0 == COMPACT_CBOR, warm_first);
        }

        #[tokio::test]
        async fn incompatible_snapshot_is_refused_before_network() {
            let (addr, _, executions) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap();
            let route = cratestack_client_rust::RouteRef::new($route, &[]);
            let call = runtime
                .prepare_call(
                    "POST",
                    route,
                    $path,
                    &echo(order()),
                    &[],
                    &[("Idempotency-Key", "key")],
                )
                .unwrap();
            let original: serde_json::Value =
                serde_json::from_slice(&call.to_bytes().unwrap()).unwrap();
            for (field, value) in [
                ("base_url", serde_json::json!("http://other.invalid/")),
                ("path", serde_json::json!("/wrong")),
                ("content_type", serde_json::json!("text/plain")),
                ("accept", serde_json::json!("text/plain")),
                ("sealed", serde_json::json!(true)),
                ("version", serde_json::json!(2)),
                ("key_tables", serde_json::json!([0])),
            ] {
                let mut changed = original.clone();
                changed[field] = value;
                let restored = cratestack_client_rust::PreparedCall::from_bytes(
                    &serde_json::to_vec(&changed).unwrap(),
                );
                if let Ok(restored) = restored {
                    assert!(
                        runtime.execute_prepared(&restored).await.is_err(),
                        "{field}"
                    );
                }
            }
            assert_eq!(executions.load(Ordering::SeqCst), 0);
        }
        #[tokio::test]
        async fn malformed_cbor_snapshots_never_send() {
            let (addr, _, executions) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap();
            let call = runtime
                .prepare_call(
                    "POST",
                    cratestack_client_rust::RouteRef::new($route, &[]),
                    $path,
                    &echo(order()),
                    &[],
                    &[("Idempotency-Key", "k")],
                )
                .unwrap();
            let original: serde_json::Value =
                serde_json::from_slice(&call.to_bytes().unwrap()).unwrap();
            for body in [
                vec![],
                vec![0, 0],
                vec![0x1c],
                vec![0xff],
                vec![0x81, 0xff],
                vec![0xbf, 0, 0xff],
                vec![0xf8, 0],
                vec![0x61, 0xff],
                vec![0x62, 0xc0, 0x80],
                vec![0x63, 0xed, 0xa0, 0x80],
                vec![0x64, 0xf4, 0x90, 0x80, 0x80],
                vec![0x7f, 0x61, 0xc2, 0x61, 0xa2, 0xff],
            ] {
                let mut changed = original.clone();
                changed["body"] = serde_json::json!(body);
                assert!(
                    cratestack_client_rust::PreparedCall::from_bytes(
                        &serde_json::to_vec(&changed).unwrap()
                    )
                    .is_err()
                );
            }
            assert_eq!(executions.load(Ordering::SeqCst), 0);
        }

        #[tokio::test]
        async fn integer_key_map_prepares_and_restores_without_json_conversion() {
            let (addr, _, executions) = spawn().await;
            let runtime = CratestackClient::cbor(ClientConfig::new(
                reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
            ))
            .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
            .unwrap();
            let input = std::collections::BTreeMap::from([(1_u8, 2_u8)]);
            let call = runtime
                .prepare_call(
                    "POST",
                    cratestack_client_rust::RouteRef::new($route, &[]),
                    $path,
                    &input,
                    &[],
                    &[("Idempotency-Key", "integer-map")],
                )
                .unwrap();
            let restored =
                cratestack_client_rust::PreparedCall::from_bytes(&call.to_bytes().unwrap())
                    .unwrap();
            assert_eq!(restored.payload(), call.payload());
            assert_eq!(executions.load(Ordering::SeqCst), 0);
        }

        #[tokio::test]
        async fn persisted_named_replays_after_restart_and_pressure() {
            prepared_replay(false).await;
        }
        #[tokio::test]
        async fn persisted_compact_replays_after_restart_and_pressure() {
            prepared_replay(true).await;
        }
    };
}
