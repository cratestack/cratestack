//! Response-header blob: encode/decode the persisted header set so a
//! replay reproduces the original handler's `Location`, `ETag`, cache
//! directives, `Content-Type`, etc.

/// Headers excluded from the replay blob. `Date` should always reflect
/// when the response is actually emitted; `Content-Length` is recomputed
/// by the framework from the buffered body so capturing it would risk
/// mismatch with `Vec<u8>::len()` on the path back out. `Connection` and
/// `Transfer-Encoding` are hop-by-hop and meaningless to replay.
const HEADERS_NEVER_REPLAYED: &[&str] =
    &["content-length", "connection", "transfer-encoding", "date"];

fn is_replayable_header(name: &http::HeaderName) -> bool {
    !HEADERS_NEVER_REPLAYED.contains(&name.as_str())
}

/// Encode a response's headers into the opaque blob that the store
/// persists. Format: little-endian length-prefixed `(name, value)` pairs.
/// Header values can carry arbitrary bytes (per RFC 9110 they may include
/// any opaque-data octet, with the exception of CR/LF), so a binary blob
/// is the only correct representation — JSON would force lossy UTF-8
/// coercion on values like opaque `ETag` tokens that may already be
/// quoted-string blobs.
pub fn encode_headers(headers: &http::HeaderMap) -> Vec<u8> {
    let mut iter = headers
        .iter()
        .filter(|(name, _)| is_replayable_header(name));
    // Two passes so we can write the count up front; HeaderMap iter
    // doesn't expose a stable count that excludes filtered entries.
    let pairs: Vec<_> = iter.by_ref().collect();
    let mut blob = Vec::with_capacity(4 + pairs.len() * 16);
    let count = pairs.len() as u32;
    blob.extend_from_slice(&count.to_le_bytes());
    for (name, value) in pairs {
        let name_bytes = name.as_str().as_bytes();
        let value_bytes = value.as_bytes();
        blob.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        blob.extend_from_slice(name_bytes);
        blob.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        blob.extend_from_slice(value_bytes);
    }
    blob
}

/// Decode a blob produced by [`encode_headers`] back into a `HeaderMap`.
/// Returns an empty map on malformed input rather than failing the
/// replay — a corrupt headers blob is a recoverable curiosity, not a
/// reason to drop the response status and body the caller is waiting
/// for.
pub fn decode_headers(blob: &[u8]) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    if blob.is_empty() {
        return headers;
    }
    let mut cursor = 0;
    let read_u32 = |bytes: &[u8], offset: usize| -> Option<usize> {
        bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4-byte slice")) as usize)
    };
    let Some(count) = read_u32(blob, cursor) else {
        return headers;
    };
    cursor += 4;
    for _ in 0..count {
        let Some(name_len) = read_u32(blob, cursor) else {
            return headers;
        };
        cursor += 4;
        let Some(name_bytes) = blob.get(cursor..cursor + name_len) else {
            return headers;
        };
        cursor += name_len;
        let Some(value_len) = read_u32(blob, cursor) else {
            return headers;
        };
        cursor += 4;
        let Some(value_bytes) = blob.get(cursor..cursor + value_len) else {
            return headers;
        };
        cursor += value_len;
        let Ok(name) = http::HeaderName::from_bytes(name_bytes) else {
            continue;
        };
        let Ok(value) = http::HeaderValue::from_bytes(value_bytes) else {
            continue;
        };
        // `append`, not `insert`: preserves multi-valued headers like
        // `Set-Cookie` exactly as the handler emitted them.
        headers.append(name, value);
    }
    headers
}
