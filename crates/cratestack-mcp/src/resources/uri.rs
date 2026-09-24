//! Matching a `resources/read` URI against the generated table.
//!
//! Strict on purpose. A URI is either exactly a record URI, exactly a
//! collection URI with only `limit`/`cursor` in its query, or unknown.
//! Nothing is normalized (no case folding of the authority, no trailing
//! slash, no fragment): a lenient matcher is where two spellings of "the
//! same" resource start behaving differently.

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
    /// one (bad percent-encoding). Answered exactly like a missing row.
    Unknown,
    /// A known collection with a query this server cannot honour. The
    /// message restates only the caller's own input.
    Invalid(String),
}

pub(crate) fn parse<'t>(
    uri: &str,
    table: &'t [ResourceDescriptor],
) -> Result<Target<'t>, UriError> {
    let rest = uri
        .strip_prefix(RESOURCE_SCHEME)
        .and_then(|rest| rest.strip_prefix("://"))
        .ok_or(UriError::Unknown)?;
    if rest.contains('#') {
        return Err(UriError::Unknown);
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let mut parts = path.split('/');
    let (Some(schema), Some(segment)) = (parts.next(), parts.next()) else {
        return Err(UriError::Unknown);
    };
    let id = parts.next();
    if parts.next().is_some() {
        return Err(UriError::Unknown);
    }
    let resource = table
        .iter()
        .find(|resource| resource.schema == schema && resource.segment == segment)
        .ok_or(UriError::Unknown)?;

    match id {
        Some(raw) => {
            if query.is_some() {
                return Err(UriError::Invalid(
                    "a record URI takes no query; paging applies to the collection URI".to_owned(),
                ));
            }
            let id = percent_decode(raw).ok_or(UriError::Unknown)?;
            // A NUL is in no key: `Int`/`Uuid` never parse one, and
            // Postgres refuses it in `text` with an error rather than
            // matching nothing — a `-32603` and a server-side error log
            // any caller could trigger at will, where the true answer is
            // "no such row". Deliberately unlike REST, which 500s.
            if id.is_empty() || id.contains('\0') {
                return Err(UriError::Unknown);
            }
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

/// RFC 3986 percent-decoding into UTF-8. `None` for a malformed escape or
/// bytes that are not UTF-8.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = raw.get(at + 1..at + 3)?;
            // `from_str_radix` alone would accept a sign (`%+f`).
            if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
