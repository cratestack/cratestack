#![cfg(test)]

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::CratestackError;

#[test]
fn a_throttle_frame_is_resource_exhausted_with_retry_after() {
    let frame = SubscriptionRefusal::Throttled {
        retry_after_secs: 7,
    }
    .frame(3);
    assert_eq!(frame.id, 3);
    assert!(frame.ready.is_none());
    let err = frame.err.expect("an err frame");
    assert_eq!(err.code, "resource_exhausted");
    assert_eq!(
        err.details,
        Some(serde_json::json!({ "retry_after_secs": 7 }))
    );
}

#[test]
fn an_error_refusal_frame_carries_the_errors_code() {
    let frame =
        SubscriptionRefusal::Error(CratestackError::PreconditionFailed("no peer".to_owned()))
            .frame(9);
    let err = frame.err.expect("an err frame");
    assert_eq!(frame.id, 9);
    assert_eq!(err.code, "failed_precondition");
    assert_eq!(err.details, None);
}

#[tokio::test]
async fn a_charger_without_a_limiter_admits_everything() {
    assert!(SessionCharger::default().admit("anything").await.is_ok());
}

type Seen = Vec<(String, Option<String>, Option<IpAddr>)>;

#[derive(Debug, Default)]
struct Recording(Mutex<Seen>);

#[async_trait]
impl SubscriptionRateLimit for Recording {
    async fn admit(&self, charge: SubscriptionCharge<'_>) -> Result<(), SubscriptionRefusal> {
        self.0.lock().unwrap().push((
            charge.op_id.to_owned(),
            charge.principal.map(str::to_owned),
            charge.remote,
        ));
        Ok(())
    }
}

#[tokio::test]
async fn a_charger_hands_the_limiter_op_principal_and_remote() {
    let limiter = Arc::new(Recording::default());
    let remote = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let charger = SessionCharger::new(
        Some(limiter.clone() as Arc<dyn SubscriptionRateLimit>),
        Some("u-1".to_owned()),
        Some(remote),
    );
    assert_eq!(charger.principal(), Some("u-1"));
    charger.admit("procedure.countdown").await.unwrap();
    assert_eq!(
        *limiter.0.lock().unwrap(),
        vec![(
            "procedure.countdown".to_owned(),
            Some("u-1".to_owned()),
            Some(remote)
        )]
    );
}
