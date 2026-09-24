//! The two resource lists, behind the caller check a read passes.
//!
//! Both lists are static and unfiltered (`listing.rs`), so resolving a
//! caller changes nothing they return. It is done anyway so that, over
//! Streamable HTTP, a list answers only a request the guard authenticated,
//! failing closed exactly as a read or a tool call would
//! (`crate::streamable_http::caller`). Otherwise a way of reaching the
//! handler that skipped the guard would still be refused for reads and
//! served for lists, and nothing would notice. `tools/list` resolves the
//! caller the same way, inline in `server.rs` (maintainer decision on
//! #1040), so no list method is the exception.
//!
//! A function over the request's extensions rather than inline in
//! `server.rs` because `rmcp` does not let a test build a `RequestContext`,
//! and the refusal is the part worth a test (`tests_listed.rs`).

use rmcp::ErrorData;
use rmcp::model::Extensions;

use super::ResourceDescriptor;
use crate::server::McpServer;
use crate::table::McpTools;

pub(crate) fn listed<T: McpTools, R>(
    server: &McpServer<T>,
    extensions: &Extensions,
    list: fn(&[ResourceDescriptor]) -> R,
) -> Result<R, ErrorData> {
    server.caller.resolve_from(extensions)?;
    Ok(list(server.tools.resources()))
}
