//! [`OpExecutor`]'s construction-time choices: the `Option<Arc<dyn
//! IdempotencyStore>>` that makes "no store wired" and "`db = None`" the
//! same path, and the completion half of the reservation contract.
//!
//! Shares the counting fake with [`super::tests_admission`] rather than
//! re-declaring it: two fakes of one trait drift, and the whole reason the
//! bypass assertions are worth anything is that the same fake counts calls
//! the same way in both files.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cratestack_core::IdempotencyStore;

use super::tests_admission::{CountingStore, input, participating};
use crate::{Admission, OpAdmission, OpExecutor};

const TTL: std::time::Duration = std::time::Duration::from_secs(60);

#[tokio::test]
async fn no_store_wired_bypasses_everything() {
    let executor = OpExecutor::new(None, TTL);
    let admission = executor
        .admit(&input(participating(), Some("k")))
        .await
        .expect("a bypass cannot fail");

    assert!(
        matches!(admission, Admission::Bypass),
        "a service with no idempotency store — including every `db = None` \
         service — must run the op rather than refuse it"
    );
}

#[tokio::test]
async fn complete_and_release_are_no_ops_without_a_store() {
    let executor = OpExecutor::new(None, TTL);
    // Nothing to assert but "does not panic": there is no store to count
    // against, which is exactly the property under test.
    executor
        .complete("p", "k", uuid::Uuid::nil(), 200, &[], &[])
        .await;
    executor.release("p", "k", uuid::Uuid::nil()).await;
}

#[tokio::test]
async fn complete_and_release_reach_the_store_when_one_is_wired() {
    let store = Arc::new(CountingStore::reserving());
    let executor = OpExecutor::new(Some(store.clone() as Arc<dyn IdempotencyStore>), TTL);

    executor
        .complete("p", "k", uuid::Uuid::nil(), 201, b"headers", b"body")
        .await;
    executor.release("p", "k", uuid::Uuid::nil()).await;

    assert_eq!(store.complete_calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.release_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn unresolved_admission_fails_closed_toward_reserving() {
    let unresolved = OpAdmission::unresolved();
    assert!(
        !unresolved.idempotent_by_default,
        "an op nobody could identify must still take a reservation — the \
         inverse of the rate-limit filters' fail-closed direction, and the \
         reason installing no resolver leaves behaviour bit-for-bit as it was"
    );
    assert!(unresolved.rate_limited_by_default);
}

#[test]
fn both_descriptor_shapes_lift_onto_one_admission_type() {
    let from_rpc: OpAdmission = (&OP).into();
    let from_rest: OpAdmission = (&ROUTE).into();

    assert_eq!(
        from_rpc.idempotent_by_default,
        from_rest.idempotent_by_default
    );
    assert_eq!(
        from_rpc.rate_limited_by_default,
        from_rest.rate_limited_by_default
    );
    assert_eq!(from_rpc.op_id, "procedure.transfer");
    assert_eq!(
        from_rest.op_id, "transfer",
        "REST has no dotted op id; `name` is the closest stable identifier \
         and is documented as diagnostics-only for exactly that reason"
    );
}

static OP: cratestack_core::OpDescriptor = cratestack_core::OpDescriptor {
    op_id: "procedure.transfer",
    kind: cratestack_core::OpKind::Unary,
    input_ty: "TransferInput",
    output_ty: "Transfer",
    idempotent_by_default: false,
    rate_limited_by_default: true,
    auth_required: true,
};

static ROUTE: cratestack_core::RouteTransportDescriptor =
    cratestack_core::RouteTransportDescriptor {
        name: "transfer",
        method: "POST",
        path: "/$procs/transfer",
        capabilities: cratestack_core::RouteTransportCapabilities {
            request_types: &[],
            response_types: &[],
            default_response_type: "",
            supports_sequence_response: false,
        },
        idempotent_by_default: false,
        rate_limited_by_default: true,
    };
