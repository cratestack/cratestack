//! cratestack#1261: which refusals an idempotency layer releases, and that
//! claiming one changes nothing a client can see.

use super::*;
use crate::rpc::{RpcErrorBody, rpc_code};
use crate::value::Value;

fn step_up() -> AuthRefusal {
    AuthRefusal::unauthenticated("step-up authentication required")
        .with_member(
            "error",
            Value::String("insufficient_user_authentication".into()),
        )
        .with_details(Value::String("max_age".into()))
        .with_challenge(r#"Bearer error="insufficient_user_authentication""#)
}

fn refusals() -> Vec<CratestackError> {
    vec![
        CratestackError::Unauthorized("credential expired".to_owned()),
        CratestackError::Forbidden("procedure policy denied this operation".to_owned()),
        step_up().into_error(),
        AuthRefusal::forbidden("no").into_error(),
    ]
}

#[test]
fn a_refusal_is_recorded_until_the_dispatch_claims_it() {
    for refusal in refusals() {
        assert!(refusal.is_idempotency_replayable(), "{refusal:?}");
        let claimed = refusal.__generated_claim_auth_refusal();
        assert!(!claimed.is_idempotency_replayable(), "{claimed:?}");
        let CratestackError::AuthRefused(inner) = &claimed else {
            panic!("a claimed refusal is an AuthRefused: {claimed:?}");
        };
        assert!(inner.is_dispatch_owned());
    }
}

#[test]
fn claiming_changes_nothing_on_the_wire() {
    for refusal in refusals() {
        let (code, status, rpc) = (
            refusal.code(),
            refusal.status_code(),
            rpc_code(&refusal).to_owned(),
        );
        let (message, detail) = (
            refusal.public_message().into_owned(),
            refusal.detail().map(str::to_owned),
        );
        let challenges = refusal.auth_challenges().to_vec();
        let rpc_body = RpcErrorBody::from_cratestack(&refusal);
        let envelope = refusal.clone_for_test().into_response();

        let claimed = refusal.__generated_claim_auth_refusal();
        assert_eq!(claimed.code(), code);
        assert_eq!(claimed.status_code(), status);
        assert_eq!(rpc_code(&claimed), rpc);
        assert_eq!(claimed.public_message(), message);
        assert_eq!(claimed.detail().map(str::to_owned), detail);
        assert_eq!(claimed.auth_challenges(), challenges.as_slice());
        assert_eq!(RpcErrorBody::from_cratestack(&claimed), rpc_body);
        assert_eq!(claimed.into_response(), envelope);
    }
}

#[test]
fn a_refusal_out_of_a_body_is_disowned() {
    for refusal in refusals() {
        let disowned = refusal
            .__generated_claim_auth_refusal()
            .disown_auth_refusal();
        assert!(disowned.is_idempotency_replayable(), "{disowned:?}");
    }
}

#[test]
fn every_other_error_is_left_alone() {
    let others = [
        CratestackError::Validation("bad".to_owned()),
        CratestackError::NotFound("gone".to_owned()),
        CratestackError::Internal("provider down".to_owned()),
    ];
    for error in others {
        let code = error.code();
        let claimed = error.__generated_claim_auth_refusal();
        assert_eq!(claimed.code(), code);
        assert!(!matches!(claimed, CratestackError::AuthRefused(_)));
        assert!(claimed.is_idempotency_replayable());
        assert!(claimed.disown_auth_refusal().is_idempotency_replayable());
    }
}

impl CratestackError {
    /// The four refusal shapes above, rebuilt: `CratestackError` is not
    /// `Clone`.
    fn clone_for_test(&self) -> Self {
        match self {
            Self::Unauthorized(message) => Self::Unauthorized(message.clone()),
            Self::Forbidden(message) => Self::Forbidden(message.clone()),
            Self::AuthRefused(refusal) => Self::AuthRefused(refusal.clone()),
            other => panic!("not a refusal: {other:?}"),
        }
    }
}
