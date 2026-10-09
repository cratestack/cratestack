//! ADR 0019 D5 (PR A): a validator on a `type` field runs on a procedure
//! argument over REST, JSON and CBOR. In-process against `db = None`, so no
//! Postgres: `authorize_with_db`, where the check lives, is the same
//! generated code under `db = Postgres`. The RPC half is
//! `type_validators_rpc.rs`; both read the one table in
//! `type_validators_support`.

mod type_validators_support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;
use type_validators_support::cases;

include_server_schema!("tests/fixtures/type_validators_rest.cstack", db = None);

use cratestack_schema::procedures as p;

#[derive(Clone, Default)]
struct Procedures;

macro_rules! reply {
    ($name:ident) => {
        fn $name(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CratestackContext,
            _args: p::$name::Args,
            _authorized: p::$name::Authorized,
        ) -> impl core::future::Future<Output = Result<p::$name::Output, CratestackError>> + Send {
            async { Ok(cratestack_schema::Reply { ok: true }) }
        }
    };
}

impl p::ProcedureRegistry for Procedures {
    reply!(greet);
    reply!(open_account);
    reply!(relabel);
    reply!(plain);
}

#[derive(Clone)]
struct Authenticated;

impl cratestack::AuthProvider for Authenticated {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn router<C: cratestack::HttpTransport>(codec: C) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        codec,
        Authenticated,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn post(
    router: cratestack::axum::Router,
    content_type: &'static str,
    procedure: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let response = router
        .oneshot(
            Request::post(format!("/$procs/{procedure}"))
                .header("content-type", content_type)
                .header("accept", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

/// Every case of the shared table, against the codec `encode`/`decode`
/// speak: accepted ones answer 200, rejected ones the 422 envelope a model
/// input's failed validator also answers, with the path in the message.
macro_rules! run_table {
    ($router:expr, $codec:expr, $content_type:expr) => {
        for case in cases() {
            let body = $codec.encode(&case.body).expect("body encodes");
            let (status, bytes) = post($router, $content_type, case.procedure, body).await;
            match case.rejected {
                None => assert_eq!(status, StatusCode::OK, "{}", case.label),
                Some(message) => {
                    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{}", case.label);
                    let error: cratestack::CratestackErrorResponse =
                        $codec.decode(&bytes).expect("error envelope decodes");
                    assert_eq!(error.code, "VALIDATION_ERROR", "{}", case.label);
                    assert_eq!(error.message, message, "{}", case.label);
                }
            }
        }
    };
}

#[tokio::test]
async fn json_arguments_are_validated() {
    run_table!(router(JsonCodec), JsonCodec, JsonCodec::CONTENT_TYPE);
}

#[tokio::test]
async fn cbor_arguments_are_validated() {
    run_table!(router(CborCodec), CborCodec, CborCodec::CONTENT_TYPE);
}

/// The check is in `authorize_with_db`, which every caller of
/// `invoke_with_db` passes through, not in a transport handler: a cron job
/// or worker is validated by the same call. It precedes `@allow`, as a model
/// input's `validate` precedes its create policy, so an anonymous caller
/// learns the argument is invalid before it learns it is not allowed.
async fn call(
    db: &cratestack_schema::Cratestack,
    message: &str,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    let args = p::greet::Args {
        args: cratestack_schema::Greeting {
            message: message.to_owned(),
        },
    };
    p::greet::invoke_with_db(db, &args, ctx, |_authorized| async { Ok(()) }).await
}

#[tokio::test]
async fn a_non_http_caller_is_validated_before_policy() {
    let db = cratestack_schema::Cratestack::builder().build();
    let signed_in = CratestackContext::authenticated([("id".into(), cratestack::Value::Int(1))]);
    let anonymous = CratestackContext::anonymous();

    let invalid = call(&db, "hi", &signed_in).await.unwrap_err();
    assert!(
        matches!(&invalid, CratestackError::Validation(m)
            if m == "field 'args.message' length 2 is below minimum 3"),
        "{invalid:?}"
    );
    call(&db, "hello", &signed_in)
        .await
        .expect("valid arguments pass");

    assert!(matches!(
        call(&db, "hi", &anonymous).await.unwrap_err(),
        CratestackError::Validation(_)
    ));
    assert!(matches!(
        call(&db, "hello", &anonymous).await.unwrap_err(),
        CratestackError::Forbidden(_)
    ));
}

/// `Plain` has no validator, so its `Args` needs no impl and gets none; a `type`
/// that holds a validated one does. Compiling these calls is the assertion.
#[test]
fn a_validated_type_and_the_arguments_that_hold_one_implement_the_trait() {
    use cratestack::ValidateFields;
    fn implements<T: ValidateFields>() {}
    implements::<cratestack_schema::Greeting>();
    implements::<cratestack_schema::Owner>();
    implements::<p::greet::Args>();
    implements::<p::relabel::Args>();
    let tag = cratestack_schema::Tag { label: "x".into() };
    assert!(tag.validate().is_err());
}
