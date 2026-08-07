//! Ticket #209's load-bearing integration test — the Rust sibling of
//! `../ts-client-e2e.mjs` (ticket #172) and `../dart-client/tool/e2e.dart`
//! (ticket #210): generate a real Rust gRPC client
//! (`include_client_schema!` against `../schemas/widgets.cstack`), boot
//! the real `grpc-widgets-example` server (ticket #171's
//! `grpcurl`-verified example, unmodified), and drive it with a real
//! `tonic` channel over HTTP/2 — no mocks, no `grpcurl`.
//!
//! Requires the server already running (same precedent as the TS/Dart
//! e2e scripts — neither is self-orchestrating either):
//!
//! ```bash
//! export DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test
//! cargo run -p grpc-widgets-example   # separate shell, leave running
//! cargo test -p grpc-widgets-rust-client-example --test e2e -- --nocapture
//! ```
//!
//! Every widget this test creates uses a random `id` (the model's PK is
//! caller-supplied, not server-generated — see `schemas/widgets.cstack`)
//! so repeated runs against the same long-lived server don't collide.
//!
//! Unlike `../ts-client-e2e.mjs`/`../dart-client/tool/e2e.dart`, this is a
//! genuine `#[tokio::test]` — Rust's `cargo test` auto-discovers every
//! file under `tests/`, so it rides along in `test-ci-host`'s blanket
//! `cargo test --workspace` (`justfile`) unless it skips itself. This
//! repo's established convention for "requires an external resource that
//! CI doesn't provision" (see e.g. `cratestack-pg/tests/banking_batches.rs`)
//! is a quiet runtime skip, not `#[ignore]` — so a connection failure here
//! prints a skip message and returns rather than panicking/failing.

use std::sync::Arc;

use cratestack::client_rust::{AuthorizationRequest, ClientError, RequestAuthorizer};
use cratestack::include_client_schema;

include_client_schema!("../schemas/widgets.cstack");

struct StaticAuthIdHeader;

#[async_trait::async_trait]
impl RequestAuthorizer for StaticAuthIdHeader {
    async fn authorize(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("x-auth-id".to_owned(), "1".to_owned())])
    }
}

fn random_id() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Sub-second-resolution timestamp, not a cryptographic ID — good
    // enough to avoid collisions between test runs against the same
    // long-lived server, which is all this needs.
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the epoch")
        .as_nanos()
        % 1_000_000_000) as i64
}

#[tokio::test]
async fn full_crud_lifecycle_against_a_real_grpc_server() {
    let endpoint =
        std::env::var("GRPC_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50061".to_owned());
    let client = match cratestack_schema::grpc::Client::connect(endpoint.clone()).await {
        Ok(client) => client.with_request_authorizer(Arc::new(StaticAuthIdHeader)),
        Err(error) => {
            eprintln!(
                "skipping full_crud_lifecycle_against_a_real_grpc_server: could not connect to \
                 {endpoint} ({error}) — is `cargo run -p grpc-widgets-example` running? Not \
                 provisioned by CI's default test job, see this file's module doc."
            );
            return;
        }
    };

    let id = random_id();

    // --- create
    let created = client
        .widgets()
        .create(&cratestack_schema::inputs::CreateWidgetInput {
            id,
            name: "gizmo".to_owned(),
        })
        .await
        .expect("create should succeed");
    assert_eq!(created.id, id);
    assert_eq!(created.name, "gizmo");

    // --- get
    let fetched = client.widgets().get(&id).await.expect("get should succeed");
    assert_eq!(fetched, created);

    // --- list (finds the created widget among the page)
    let page = client
        .widgets()
        .list(&cratestack::rpc::RpcListInput {
            limit: Some(1000),
            ..Default::default()
        })
        .await
        .expect("list should succeed");
    assert!(
        page.items.iter().any(|widget| widget.id == id),
        "expected widget {id} in list page of {} item(s)",
        page.items.len()
    );

    // --- update
    let updated = client
        .widgets()
        .update(
            &id,
            &cratestack_schema::inputs::UpdateWidgetInput {
                name: Some("gizmo-v2".to_owned()),
            },
        )
        .await
        .expect("update should succeed");
    assert_eq!(updated.id, id);
    assert_eq!(updated.name, "gizmo-v2");

    // --- delete (returns the deleted record, same as REST/RPC)
    let deleted = client
        .widgets()
        .delete(&id)
        .await
        .expect("delete should succeed");
    assert_eq!(deleted.id, id);
    assert_eq!(deleted.name, "gizmo-v2");

    // --- get-after-delete: typed `GrpcClientError::Status` carrying
    // `tonic::Code::NotFound` (`cratestack_grpc::error::rpc_code_to_tonic_code`'s
    // table — `CoolError::NotFound` -> `not_found` -> `Code::NotFound`),
    // matching the TS client's `CratestackGrpcError` check in
    // `ts-client-e2e.mjs`.
    let after_delete = client.widgets().get(&id).await;
    match after_delete {
        Err(cratestack::client_rust::grpc::GrpcClientError::Status(status)) => {
            assert_eq!(status.code(), cratestack::grpc::tonic::Code::NotFound);
        }
        other => panic!("expected a NotFound gRPC status after delete, got {other:?}"),
    }

    println!("[ok] full CRUD lifecycle against a real grpc-widgets-example server (id={id})");
}
