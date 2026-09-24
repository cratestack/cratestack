//! Seed data, the two clients (MCP over stdio framing, REST over the
//! generated router) and the seed rule the tests hold both to.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::mcp::StdioServer;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tower::util::ServiceExt;

use crate::cratestack_schema::{self, Cratestack};
use crate::support::pg;

pub const POSTS: i64 = 260;

/// The seed rule, restated: even ids are `u-1`'s, multiples of 3 are
/// drafts. `u-1` reads published posts and its own drafts, so it may not
/// read the odd multiples of 3 (3, 9, 15, …): 43 of the 260 rows.
pub fn u1_may_read(id: i64) -> bool {
    let published = id % 3 != 0;
    let own = id % 2 == 0;
    published || own
}

pub fn u1_visible_posts() -> Vec<i64> {
    (1..=POSTS).filter(|id| u1_may_read(*id)).collect()
}

pub fn user(id: &str) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::String(id.to_owned()))])
}

pub async fn seeded() -> Option<(pg::TestPg, Cratestack)> {
    let test_pg = pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS mcp_res_posts, mcp_res_notes",
        "CREATE TABLE mcp_res_posts (id BIGINT PRIMARY KEY, author_id TEXT NOT NULL, \
         title TEXT NOT NULL, published BOOLEAN NOT NULL, secret_note TEXT NOT NULL)",
        "INSERT INTO mcp_res_posts (id, author_id, title, published, secret_note) \
         SELECT g, CASE WHEN g % 2 = 0 THEN 'u-1' ELSE 'u-2' END, 'post number ' || g, \
         g % 3 <> 0, 'do not leak ' || g FROM generate_series(1, 260) AS g",
        "CREATE TABLE mcp_res_notes (id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, body TEXT NOT NULL)",
        // Notes `n-001..=n-030` belong to `u-1`, `n-031..=n-035` to `u-2`.
        "INSERT INTO mcp_res_notes (id, owner_id, body) \
         SELECT 'n-' || lpad(g::text, 3, '0'), CASE WHEN g <= 30 THEN 'u-1' ELSE 'u-2' END, \
         'note ' || g FROM generate_series(1, 35) AS g",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = Cratestack::builder(test_pg.pool.clone()).build();
    Some((test_pg, db))
}

#[derive(Clone)]
pub struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// `excerpt` is the first 8 characters of the title, so REST and MCP can
/// be compared on a resolved `@computed` value.
#[derive(Clone)]
pub struct Excerpts;

impl cratestack_schema::ComputedFieldResolver for Excerpts {
    fn resolve_mcp_res_post_excerpt(
        &self,
        _db: &Cratestack,
        source: &cratestack_schema::McpResPost,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let excerpt = source.title.chars().take(8).collect();
        async move { Ok(excerpt) }
    }
}

pub fn table(db: &Cratestack) -> cratestack_schema::mcp::McpTools<NoProcedures, Excerpts> {
    cratestack_schema::mcp::tools(db.clone(), NoProcedures, Excerpts)
}

/// A stdio MCP server answering as one caller, kept open across requests.
pub struct Mcp {
    writer: DuplexStream,
    lines: Lines<BufReader<DuplexStream>>,
    next_id: u64,
}

impl Mcp {
    pub fn start(db: &Cratestack, ctx: CratestackContext) -> Self {
        let server = StdioServer::new(table(db), ctx).expect("generated table");
        let (writer, server_reader) = tokio::io::duplex(1 << 20);
        let (server_writer, reader) = tokio::io::duplex(1 << 20);
        tokio::spawn(server.serve_io(server_reader, server_writer));
        Self {
            writer,
            lines: BufReader::new(reader).lines(),
            next_id: 1,
        }
    }

    pub async fn request(
        &mut self,
        method: &str,
        mut params: serde_json::Value,
    ) -> serde_json::Value {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        let request =
            json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params });
        self.next_id += 1;
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let line = self.lines.next_line().await.unwrap().expect("an answer");
        serde_json::from_str(&line).unwrap()
    }

    pub async fn read(&mut self, uri: &str) -> serde_json::Value {
        self.request("resources/read", json!({ "uri": uri })).await
    }
}

/// The JSON document of a successful `resources/read`.
pub fn document(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("not a read result: {response}"));
    serde_json::from_str(text).unwrap()
}

pub fn item_ids(page: &serde_json::Value) -> Vec<i64> {
    page["items"]
        .as_array()
        .unwrap_or_else(|| panic!("not a page: {page}"))
        .iter()
        .map(|item| item["id"].as_i64().unwrap())
        .collect()
}

/// Authenticates REST calls from `x-caller`, into the same context shape
/// the MCP server is given.
#[derive(Clone)]
struct CallerAuth;

impl AuthProvider for CallerAuth {
    type Error = CratestackError;
    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let caller = request
            .headers
            .get("x-caller")
            .and_then(|v| v.to_str().ok());
        core::future::ready(Ok(caller.map_or_else(CratestackContext::anonymous, user)))
    }
}

/// `GET path` over the generated REST router as `caller`, JSON codec.
pub async fn rest_get(
    db: &Cratestack,
    caller: &str,
    path: &str,
) -> (StatusCode, serde_json::Value) {
    let router = cratestack_schema::axum::model_router(db.clone(), Excerpts, JsonCodec, CallerAuth);
    let response = router
        .oneshot(
            Request::get(path)
                .header("x-caller", caller)
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}
