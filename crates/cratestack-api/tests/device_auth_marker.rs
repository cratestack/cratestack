//! cratestack#1303: the device-auth marker is for the Dart generator, and the
//! macros ignore it. The same schema, six marked procedures, is accepted by
//! `include_server_schema!` (`db = None`), `include_client_schema!` and
//! `include_embedded_schema!`, and the generated server enforces what the
//! procedures' own policies say: the public legs answer an anonymous caller,
//! and `revoke`, which is not public, refuses one.

mod authn_support;

use authn_support::HeaderProvider;
use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_schema::procedures::{
    begin_enrolment, begin_renewal, confirm_enrolment, confirm_renewal, revoke_device,
    send_enrolment_code,
};
use tower::ServiceExt;

include_server_schema!("tests/fixtures/device_auth.cstack", db = None);

/// The same schema as a client contract.
mod as_client {
    cratestack::include_client_schema!("tests/fixtures/device_auth.cstack");
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn send_enrolment_code(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: send_enrolment_code::Args,
        _authorized: send_enrolment_code::Authorized,
    ) -> Result<send_enrolment_code::Output, CratestackError> {
        Ok(cratestack_schema::CodeSent {
            phone: args.args.phone,
        })
    }

    async fn begin_enrolment(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: begin_enrolment::Args,
        _authorized: begin_enrolment::Authorized,
    ) -> Result<begin_enrolment::Output, CratestackError> {
        Ok(cratestack_schema::EnrolmentChallenges {
            enrolmentId: "e-1".to_owned(),
            ambient: "challenge".to_owned(),
            userPresent: None,
        })
    }

    async fn confirm_enrolment(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: confirm_enrolment::Args,
        _authorized: confirm_enrolment::Authorized,
    ) -> Result<confirm_enrolment::Output, CratestackError> {
        Err(CratestackError::NotFound("no such enrolment".to_owned()))
    }

    async fn begin_renewal(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: begin_renewal::Args,
        _authorized: begin_renewal::Authorized,
    ) -> Result<begin_renewal::Output, CratestackError> {
        Ok(cratestack_schema::RenewalChallenge {
            challengeId: "c-1".to_owned(),
            challenge: args.args.deviceId,
        })
    }

    async fn confirm_renewal(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: confirm_renewal::Args,
        _authorized: confirm_renewal::Authorized,
    ) -> Result<confirm_renewal::Output, CratestackError> {
        Ok(cratestack_schema::Renewed {
            attestation: "a.b.c".to_owned(),
        })
    }

    async fn revoke_device(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: revoke_device::Args,
        _authorized: revoke_device::Authorized,
    ) -> Result<revoke_device::Output, CratestackError> {
        Ok(cratestack_schema::Revoked {
            deviceId: args.args.deviceId,
        })
    }
}

async fn post(path: &str, body: serde_json::Value, caller: &[(&str, &str)]) -> StatusCode {
    let app = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        CborCodec,
        HeaderProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let mut request = Request::post(path)
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", CborCodec::CONTENT_TYPE);
    for (name, value) in caller {
        request = request.header(*name, *value);
    }
    let body = CborCodec.encode(&body).unwrap();
    let response = app
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    status
}

#[tokio::test]
async fn the_public_legs_answer_an_anonymous_caller_as_written() {
    let device = serde_json::json!({ "args": { "deviceId": "dev-1", "role": "ambient" } });
    assert_eq!(
        post("/rpc/procedure.beginRenewal", device, &[]).await,
        StatusCode::OK
    );
    let code = serde_json::json!({ "args": { "phone": "+237600000000" } });
    assert_eq!(
        post("/rpc/procedure.sendEnrolmentCode", code, &[]).await,
        StatusCode::OK
    );
    // The handler's own answer, untouched by the marker.
    let confirm = serde_json::json!({ "args": { "enrolmentId": "e-1", "ambient": "sig" } });
    assert_eq!(
        post("/rpc/procedure.confirmEnrolment", confirm, &[]).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn revoke_is_not_public_and_the_marker_does_not_change_that() {
    let revoke = serde_json::json!({ "args": { "deviceId": "dev-1" } });
    assert_eq!(
        post("/rpc/procedure.revokeDevice", revoke.clone(), &[]).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post("/rpc/procedure.revokeDevice", revoke, &[("x-sub", "usr-1")]).await,
        StatusCode::OK
    );
}
