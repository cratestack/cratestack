//! Read-only MCP resources for `@@mcp(resource: "<segment>")` models (ADR
//! 0002 § Resources, cratestack#1040).
//!
//! # URIs
//!
//! ```text
//! cratestack://<name>/<segment>/{id}             one record
//! cratestack://<name>/<segment>{?limit,cursor}   a page of records
//! ```
//!
//! `<segment>` is the author's `@@mcp(resource: ...)` value and `<name>`
//! the schema's `mcp { name = "..." }` (maintainer decision on #1040), so
//! neither a table nor a model name reaches an agent (security requirement
//! 11). The scheme is matched without regard to case, as RFC 3986 § 3.1
//! requires; the name, segment and id are matched exactly (`uri.rs`).
//!
//! # Where the row policy is enforced
//!
//! Not here. This module parses, admits and renders; the rows come from the
//! generated [`McpTools::read_record`]/[`McpTools::read_page`], which call
//! the ORM REST's own handlers call (`find_unique`, and the REST list
//! builder) under the caller's context, so `@@allow("read", ...)` is in the
//! SQL (security requirement 2).
//!
//! # Why collections are one JSON document
//!
//! MCP's `resources/read` has no cursor parameter, and an empty `contents`
//! array is forbidden, so a page cannot be "one content block per record":
//! the last, empty page would be invalid. A collection read answers one
//! `application/json` block, `{"items": [...], "nextCursor": "..."}`, with
//! `nextCursor` absent on the last page. The paging controls ride in the
//! URI's query, which is what the collection template advertises.
//!
//! [`McpTools::read_record`]: crate::McpTools::read_record
//! [`McpTools::read_page`]: crate::McpTools::read_page

mod cursor;
mod descriptor;
mod error;
mod listed;
mod listing;
mod page;
mod read;
mod uri;

#[cfg(test)]
mod tests_cursor;
#[cfg(test)]
mod tests_descriptor;
#[cfg(test)]
mod tests_error;
#[cfg(test)]
mod tests_listed;
#[cfg(test)]
mod tests_page;
#[cfg(test)]
mod tests_uri;

pub use descriptor::ResourceDescriptor;
pub use page::DEFAULT_PAGE_SIZE;

pub(crate) use listed::listed;
pub(crate) use listing::{list_resources, list_templates};
pub(crate) use read::read_resource;

/// The URI scheme every CrateStack resource uses.
pub const RESOURCE_SCHEME: &str = "cratestack";

/// The MIME type of every resource's single content block.
pub(crate) const JSON_MIME: &str = "application/json";
