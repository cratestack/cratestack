//! Matching a `resources/read` URI against the generated table.
//!
//! Strict on purpose. A URI is either exactly a record URI, exactly a
//! collection URI with only `limit`/`cursor` in its query, or unknown.
//! Nothing after the scheme is normalized (no case folding of the name,
//! segment or id, no trailing slash, no fragment): a lenient matcher is
//! where two spellings of "the same" resource start behaving differently.
//!
//! **The scheme is the one exception** (maintainer decision on #1040): RFC
//! 3986 § 3.1 makes schemes case-insensitive, so `CRATESTACK://blog/...`
//! is the same URI as `cratestack://blog/...` to any conforming client,
//! and refusing it would be this server disagreeing with the standard, not
//! strictness. The name after `://` is ours, a lowercase DNS label by the
//! parser's rule, and stays exact.

use super::id::record_id;
use super::{RESOURCE_SCHEME, ResourceDescriptor};

/// What a URI addresses.
#[derive(Debug)]
pub(crate) enum Target<'t> {
    Record {
        resource: &'t ResourceDescriptor,
        /// Percent-decoded. The generated code parses it as the primary key.
        id: String,
    },
    Page {
        resource: &'t ResourceDescriptor,
        /// The URI's `limit`, before clamping. At least 1; a value too large
        /// for `u64` saturates, since it is clamped anyway.
        limit: Option<u64>,
        cursor: Option<String>,
    },
}

/// Why a URI addresses nothing readable.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UriError {
    /// Not one of this server's resources, or a record id that cannot be
    /// one (a raw non-URI character, bad percent-encoding, a NUL; `id.rs`).
    /// Answered exactly like a missing row.
    Unknown,
    /// A known collection with a query this server cannot honour. The
    /// message restates only the caller's own input.
    Invalid(String),
}

pub(crate) fn parse<'t>(
    uri: &str,
    table: &'t [ResourceDescriptor],
) -> Result<Target<'t>, UriError> {
    let rest = strip_scheme(uri).ok_or(UriError::Unknown)?;
    if rest.contains('#') {
        return Err(UriError::Unknown);
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let mut parts = path.split('/');
    let (Some(name), Some(segment)) = (parts.next(), parts.next()) else {
        return Err(UriError::Unknown);
    };
    let id = parts.next();
    if parts.next().is_some() {
        return Err(UriError::Unknown);
    }
    let resource = table
        .iter()
        .find(|resource| resource.name == name && resource.segment == segment)
        .ok_or(UriError::Unknown)?;

    match id {
        Some(raw) => {
            if query.is_some() {
                return Err(UriError::Invalid(
                    "a record URI takes no query; paging applies to the collection URI".to_owned(),
                ));
            }
            // A raw non-URI character, a bad escape, an empty id or a NUL
            // addresses no record (`id.rs`).
            let id = record_id(raw).ok_or(UriError::Unknown)?;
            Ok(Target::Record { resource, id })
        }
        None => {
            let (limit, cursor) = page_query(query)?;
            Ok(Target::Page {
                resource,
                limit,
                cursor,
            })
        }
    }
}

/// What follows `cratestack://`, the scheme compared ASCII
/// case-insensitively (RFC 3986 § 3.1). `get` rather than slicing, so a
/// multi-byte character where the scheme would end is a mismatch, not a
/// panic.
fn strip_scheme(uri: &str) -> Option<&str> {
    let scheme = uri.get(..RESOURCE_SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(RESOURCE_SCHEME) {
        return None;
    }
    uri[RESOURCE_SCHEME.len()..].strip_prefix("://")
}

type PageQuery = (Option<u64>, Option<String>);

fn page_query(query: Option<&str>) -> Result<PageQuery, UriError> {
    let (mut limit, mut cursor) = (None, None);
    let Some(query) = query else {
        return Ok((limit, cursor));
    };
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "limit" if limit.is_none() => limit = Some(parse_limit(value)?),
            "cursor" if cursor.is_none() => {
                if value.is_empty() {
                    return Err(invalid("`cursor` must not be empty"));
                }
                cursor = Some(value.to_owned());
            }
            "limit" | "cursor" => return Err(invalid(format!("`{key}` appears twice"))),
            other => {
                return Err(invalid(format!(
                    "unknown query parameter `{other}`; a collection takes `limit` and `cursor`"
                )));
            }
        }
    }
    Ok((limit, cursor))
}

fn parse_limit(value: &str) -> Result<u64, UriError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("`limit` must be a positive integer"));
    }
    // Only digits, so the one way to fail is overflow: saturate, because
    // any value that large is clamped to the ceiling anyway.
    let limit = value.parse::<u64>().unwrap_or(u64::MAX);
    if limit == 0 {
        return Err(invalid("`limit` must be a positive integer"));
    }
    Ok(limit)
}

fn invalid(message: impl Into<String>) -> UriError {
    UriError::Invalid(message.into())
}
