//! [`OpExecutor::admit`]: which inputs reach the store at all, and how the
//! four store outcomes map onto [`Admission`].
//!
//! The fake below counts calls rather than asserting on them inline,
//! because the interesting property of a bypass is *negative* — the store
//! was never asked. An assertion that a reserve returned `Bypass` would
//! also pass against an implementation that reserved first and threw the
//! answer away.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::idempotency_record::{IdempotencyRecord, ReservationOutcome};
use cratestack_core::{CratestackError, IdempotencyStore};

use crate::{Admission, OpAdmission, OpExecutor, OpInput};

pub(crate) struct CountingStore {
    pub(crate) reserve_calls: AtomicUsize,
    pub(crate) complete_calls: AtomicUsize,
    pub(crate) release_calls: AtomicUsize,
    outcome: ReservationOutcome,
}

impl CountingStore {
    pub(crate) fn new(outcome: ReservationOutcome) -> Self {
        Self {
            reserve_calls: AtomicUsize::new(0),
            complete_calls: AtomicUsize::new(0),
            release_calls: AtomicUsize::new(0),
            outcome,
        }
    }

    pub(crate) fn reserving() -> Self {
        Self::new(ReservationOutcome::Reserved {
            token: uuid::Uuid::nil(),
        })
    }
}

#[async_trait]
impl IdempotencyStore for CountingStore {
    async fn reserve_or_fetch(
        &self,
        _principal: &str,
        _key: &str,
        _request_hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        self.reserve_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.outcome.clone())
    }

    async fn complete(
        &self,
        _principal: &str,
        _key: &str,
        _token: uuid::Uuid,
        _status: u16,
        _headers: &[u8],
        _body: &[u8],
    ) -> Result<(), CratestackError> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn release(
        &self,
        _principal: &str,
        _key: &str,
        _token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        self.release_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

pub(crate) fn input<'a>(op: OpAdmission, key: Option<&'a str>) -> OpInput<'a> {
    OpInput {
        op,
        principal: "principal-fp",
        idempotency_key: key,
        fingerprint: [7u8; 32],
        ctx: None,
    }
}

pub(crate) fn participating() -> OpAdmission {
    OpAdmission {
        diagnostic_op_id: "procedure.transfer",
        idempotent_by_default: false,
        rate_limited_by_default: true,
    }
}

pub(crate) fn opted_out() -> OpAdmission {
    OpAdmission {
        diagnostic_op_id: "procedure.transfer",
        idempotent_by_default: true,
        rate_limited_by_default: true,
    }
}

fn executor(store: &Arc<CountingStore>) -> OpExecutor {
    OpExecutor::new(
        Some(store.clone() as Arc<dyn IdempotencyStore>),
        std::time::Duration::from_secs(60),
    )
}

#[tokio::test]
async fn participating_op_with_a_key_reserves_exactly_once() {
    let store = Arc::new(CountingStore::reserving());
    let admission = executor(&store)
        .admit(&input(participating(), Some("k")))
        .await
        .expect("store is infallible in this fake");

    assert!(matches!(admission, Admission::Reserved { .. }));
    assert_eq!(store.reserve_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn idempotent_by_default_op_bypasses_without_touching_the_store() {
    let store = Arc::new(CountingStore::reserving());
    let admission = executor(&store)
        .admit(&input(opted_out(), Some("k")))
        .await
        .expect("bypass is not fallible");

    assert!(matches!(admission, Admission::Bypass));
    assert_eq!(
        store.reserve_calls.load(Ordering::SeqCst),
        0,
        "an op that opts out of idempotency must not reserve at all — a \
         reservation taken and discarded still costs a row and still \
         conflicts with a concurrent caller"
    );
}

#[tokio::test]
async fn a_missing_key_bypasses_without_touching_the_store() {
    let store = Arc::new(CountingStore::reserving());
    let admission = executor(&store)
        .admit(&input(participating(), None))
        .await
        .expect("bypass is not fallible");

    assert!(matches!(admission, Admission::Bypass));
    assert_eq!(store.reserve_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn every_store_outcome_maps_to_its_namesake() {
    let record = IdempotencyRecord {
        key: "k".to_owned(),
        principal_fingerprint: "principal-fp".to_owned(),
        request_hash: [7u8; 32],
        response_status: 201,
        response_headers: Vec::new(),
        response_body: Vec::new(),
        created_at: SystemTime::UNIX_EPOCH,
        expires_at: SystemTime::UNIX_EPOCH,
    };
    for (outcome, expected) in [
        (ReservationOutcome::Replay(record), "replay"),
        (ReservationOutcome::InFlight, "in-flight"),
        (ReservationOutcome::Conflict, "conflict"),
    ] {
        let store = Arc::new(CountingStore::new(outcome));
        let admission = executor(&store)
            .admit(&input(participating(), Some("k")))
            .await
            .expect("store is infallible in this fake");
        let actual = match admission {
            Admission::Replay(_) => "replay",
            Admission::InFlight => "in-flight",
            Admission::Conflict => "conflict",
            Admission::Reserved { .. } => "reserved",
            Admission::Bypass => "bypass",
        };
        assert_eq!(actual, expected);
    }
}
