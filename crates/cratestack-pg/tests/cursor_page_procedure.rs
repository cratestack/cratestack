//! Built-in `CursorPageInput` / `CursorPage<T>` procedure types
//! (cratestack#1304): camelCase wire shape, `MAX_LIST_LIMIT`-style clamping,
//! and an end-to-end cursor walk over HTTP using the same primitives the
//! generated list route uses. Postgres-free (`db = None`).

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError, CursorPage, CursorPageInput};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/cursor_page_procedure.cstack", db = None);

use cratestack_schema::Entry;

/// 1..=7 in id order; a hand-written keyset over it.
#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn feed(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::feed::Args,
        _authorized: cratestack_schema::procedures::feed::Authorized,
    ) -> Result<cratestack_schema::procedures::feed::Output, CratestackError> {
        let (limit, cursor) = args.page.resolve(3);
        let after = match cursor {
            Some(cursor) => {
                let values = cratestack::decode_cursor(cursor, "id", 1)?;
                values[0]
                    .as_deref()
                    .and_then(|v| v.parse::<i64>().ok())
                    .ok_or_else(|| CratestackError::BadRequest("invalid cursor".to_owned()))?
            }
            None => 0,
        };
        let rows: Vec<Entry> = (after + 1..=7)
            .take(usize::try_from(limit + 1).unwrap())
            .map(|id| Entry {
                id,
                title: format!("entry-{id}"),
            })
            .collect();
        Ok(CursorPage::from_rows(rows, limit, |last| {
            cratestack::encode_cursor("id", &[Some(last.id.to_string())])
        }))
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
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

fn router() -> cratestack::axum::Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn call(page: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let response = router()
        .oneshot(
            Request::post("/$procs/feed")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "page": page })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[test]
fn cursor_page_input_decodes_from_camel_case_json() {
    let args: cratestack_schema::procedures::feed::Args =
        serde_json::from_value(serde_json::json!({ "page": { "limit": 10, "cursor": "abc" } }))
            .expect("CursorPageInput arg should decode from camelCase JSON");
    assert_eq!(
        args.page,
        CursorPageInput {
            limit: Some(10),
            cursor: Some("abc".to_owned())
        }
    );
}

#[tokio::test]
async fn cursor_page_procedure_walks_pages_over_http() {
    let mut ids = Vec::new();
    let mut page = serde_json::json!({ "limit": 3 });
    loop {
        let (status, reply) = call(page).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        ids.extend(
            reply["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_i64().unwrap()),
        );
        match reply["nextCursor"].as_str() {
            Some(cursor) => page = serde_json::json!({ "limit": 3, "cursor": cursor }),
            None => break,
        }
    }
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7]);

    // The last page omits `nextCursor`; a bad cursor is a 400.
    let (status, _) = call(serde_json::json!({ "cursor": "bogus" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
