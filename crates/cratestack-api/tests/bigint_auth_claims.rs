//! ADR 0019 (PR B, risk 2): a `BigInt` auth claim decides a procedure policy
//! only when it is an integer. A string claim, canonical or not, is neither
//! equal to a `BigInt` argument nor different from it, so `!=` and `==` both
//! refuse it, and a `@deny` on either fires.
//!
//! A claim above 2^53 has to be a string to survive a JavaScript issuer, and a
//! `BigInt` argument is always an integer. The evaluator used to call the
//! string `"7"` different from `7`: `owner != auth().accountId` passed for the
//! caller it was written to refuse. The SQL path refuses the same claim
//! (`$N::text` against `bigint`), and the two must agree
//! (`cratestack-policy/src/compare.rs`). In-process against `db = None`, REST,
//! over both codecs; the claim is parsed with `serde_json` from an `x-claim`
//! header, which is how a real provider turns a token's JSON into a `Value`.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{
    CratestackCodec, CratestackContext, CratestackError, Value, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use serde_json::json;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/bigint_auth_claims.cstack", db = None);

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
            async { Ok(cratestack_schema::Receipt { ok: true }) }
        }
    };
}

impl p::ProcedureRegistry for Procedures {
    reply!(not_mine);
    reply!(mine);
    reply!(not_mine_deny);
}

/// Authenticates the request whose `x-claim` header holds the JSON of the
/// `accountId` claim; a request without one is anonymous.
#[derive(Clone)]
struct ClaimFromHeader;

impl cratestack::AuthProvider for ClaimFromHeader {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let context = match request.headers.get("x-claim") {
            Some(raw) => {
                let claim: Value =
                    serde_json::from_slice(raw.as_bytes()).expect("the claim is JSON");
                CratestackContext::authenticated([("accountId".to_owned(), claim)])
            }
            None => CratestackContext::anonymous(),
        };
        core::future::ready(Ok(context))
    }
}

fn router<C: cratestack::HttpTransport>(codec: C) -> cratestack::axum::Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        codec,
        ClaimFromHeader,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn status<C: cratestack::HttpTransport + CratestackCodec>(
    codec: C,
    content_type: &'static str,
    case: &Case,
) -> StatusCode {
    let mut request = Request::post(format!("/$procs/{}", case.procedure))
        .header("content-type", content_type)
        .header("accept", content_type);
    if let Some(claim) = &case.claim {
        request = request.header("x-claim", claim.as_str());
    }
    let body = codec
        .encode(&json!({ "owner": case.owner }))
        .expect("body encodes");
    let response = router(codec)
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    status
}

struct Case {
    procedure: &'static str,
    owner: String,
    /// The claim as JSON text, `None` for an anonymous caller.
    claim: Option<String>,
    expected: StatusCode,
}

fn case(procedure: &'static str, owner: &str, claim: Option<&str>, ok: bool) -> Case {
    Case {
        procedure,
        owner: owner.to_owned(),
        claim: claim.map(str::to_owned),
        expected: if ok {
            StatusCode::OK
        } else {
            StatusCode::FORBIDDEN
        },
    }
}

/// `(a number, a different number beside it)`. 2^53 + 1 sits next to 2^53,
/// the integer an `f64` round trip would turn it into.
const NUMBERS: [(&str, &str); 3] = [
    ("7", "8"),
    ("9007199254740993", "9007199254740992"),
    ("9223372036854775807", "9223372036854775806"),
];

/// Claims that are strings, as JSON text: canonical, signed, padded, a
/// fraction, whitespace, empty, a word, and a canonical string past `i64`.
/// None is a number to the evaluator.
const STRING_CLAIMS: [&str; 8] = [
    r#""7""#,
    r#""+7""#,
    r#""007""#,
    r#""7.0""#,
    r#"" 7""#,
    r#""""#,
    r#""seven""#,
    r#""9223372036854775808""#,
];

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (n, other) in NUMBERS {
        // An integer claim decides every policy.
        let claim = Some(n);
        cases.push(case("notMine", n, claim, false));
        cases.push(case("notMine", other, claim, true));
        cases.push(case("mine", n, claim, true));
        cases.push(case("mine", other, claim, false));
        cases.push(case("notMineDeny", n, claim, false));
        cases.push(case("notMineDeny", other, claim, true));
        // The same number as a string decides none of them, whichever owner.
        let quoted = format!("\"{n}\"");
        for owner in [n, other] {
            for procedure in ["notMine", "mine", "notMineDeny"] {
                cases.push(case(procedure, owner, Some(quoted.as_str()), false));
            }
        }
    }
    for claim in STRING_CLAIMS {
        for owner in ["7", "8"] {
            for procedure in ["notMine", "mine", "notMineDeny"] {
                cases.push(case(procedure, owner, Some(claim), false));
            }
        }
    }
    // No claim at all satisfies neither `==` nor `!=`.
    cases.push(case("notMine", "7", None, false));
    cases.push(case("mine", "7", None, false));
    cases
}

async fn run<C: cratestack::HttpTransport + CratestackCodec>(
    codec: C,
    content_type: &'static str,
    label: &str,
) {
    for case in cases() {
        let got = status(codec.clone(), content_type, &case).await;
        assert_eq!(
            got, case.expected,
            "{label} {} owner {} claim {:?}",
            case.procedure, case.owner, case.claim
        );
    }
}

#[tokio::test]
async fn json_decides_a_bigint_claim() {
    run(JsonCodec, JsonCodec::CONTENT_TYPE, "JSON").await;
}

#[tokio::test]
async fn cbor_decides_a_bigint_claim() {
    run(CborCodec, CborCodec::CONTENT_TYPE, "CBOR").await;
}
