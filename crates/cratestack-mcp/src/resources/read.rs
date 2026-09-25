//! One `resources/read`, in ADR 0002 § Dispatch's order:
//!
//! ```text
//! match the URI against the table      unknown        -> -32602 "resource not found"
//! validate limit / cursor              malformed      -> -32602, names the parameter
//! L3 admission                         rate limit, as a tool call's (src/admission.rs)
//! generated read under the caller's ctx   @@allow in the SQL
//! render                               not visible    -> the same -32602 as unknown
//! ```
//!
//! Validation comes before admission for the reason tool arguments decode
//! first: a request that can never be served must not charge a token.
//! Reads take no idempotency reservation; the descriptors say so
//! (`idempotent_by_default: true`) and there is nothing to replay.

use cratestack_core::CratestackContext;
use cratestack_exec::OpAdmission;
use rmcp::ErrorData;
use rmcp::model::{CacheScope, ReadResourceResult, ResourceContents};
use serde_json::{Map, Value};

use super::uri::{Target, UriError, parse};
use super::{JSON_MIME, cursor, error, page};
use crate::admission::rate_limit;
use crate::server::McpServer;
use crate::table::McpTools;

pub(crate) async fn read_resource<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    uri: &str,
) -> Result<ReadResourceResult, ErrorData> {
    let target = match parse(uri, server.tools.resources()) {
        Ok(target) => target,
        Err(UriError::Unknown) => return Err(error::not_found()),
        Err(UriError::Invalid(message)) => return Err(error::invalid(message)),
    };
    let body = match target {
        Target::Record { resource, id } => {
            admit(server, ctx, resource.get_op, uri).await?;
            match server.tools.read_record(resource.segment, &id, ctx).await {
                Ok(Some(record)) => record,
                Ok(None) => return Err(error::not_found()),
                Err(failure) => return Err(error::from_cratestack(uri, failure)),
            }
        }
        Target::Page {
            resource,
            limit,
            cursor: raw_cursor,
        } => {
            let offset = match raw_cursor {
                None => 0,
                Some(raw) => cursor::decode(resource.segment, &raw).ok_or_else(|| {
                    error::invalid("`cursor` is not a cursor this server issued for this resource")
                })?,
            };
            let size = page::page_size(limit, resource.max_page_size);
            admit(server, ctx, resource.list_op, uri).await?;
            // One row past the page says whether another page exists
            // without a `COUNT(*)`: a count of visible rows is harmless,
            // but it is a second query that could drift from this one.
            let mut items = server
                .tools
                .read_page(resource.segment, size + 1, offset, ctx)
                .await
                .map_err(|failure| error::from_cratestack(uri, failure))?;
            let more = items.len() > size as usize;
            items.truncate(size as usize);
            let mut page = Map::new();
            page.insert("items".to_owned(), Value::Array(items));
            if let Some(next) = offset.checked_add(u64::from(size)).filter(|_| more) {
                page.insert(
                    "nextCursor".to_owned(),
                    Value::String(cursor::encode(resource.segment, next)),
                );
            }
            Value::Object(page)
        }
    };
    tracing::info!(
        target: "cratestack",
        cratestack_operation = "mcp_resource_read",
        "cratestack mcp resource read completed",
    );
    Ok(rendered(uri, &body))
}

/// Charged to `ctx`, the same caller the read then runs as: one bucket per
/// principal, shared with that principal's tool calls.
async fn admit<T: McpTools>(
    server: &McpServer<T>,
    ctx: &CratestackContext,
    op: &'static cratestack_core::OpDescriptor,
    uri: &str,
) -> Result<(), ErrorData> {
    rate_limit(server, ctx, OpAdmission::from(op))
        .await
        .map_err(|failure| error::from_cratestack(uri, failure))
}

/// One JSON block. `ttlMs: 0`: rows change whenever the application writes
/// them, and nothing here knows when that is, so no result is fresh past
/// the moment it was read. `cacheScope: private`: what a caller sees is
/// decided by its own row policy, so one caller's result is never valid
/// for another (both required by 2026-07-28).
fn rendered(uri: &str, body: &Value) -> ReadResourceResult {
    let contents = ResourceContents::TextResourceContents {
        uri: uri.to_owned(),
        mime_type: Some(JSON_MIME.to_owned()),
        text: body.to_string(),
        meta: None,
    };
    ReadResourceResult::new(vec![contents])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
}
