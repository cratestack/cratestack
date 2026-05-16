//! Headers blob round-trip tests.

#![cfg(test)]

use super::headers::{decode_headers, encode_headers};

#[test]
fn encode_then_decode_round_trips_headers_with_multi_values() {
    let mut headers = http::HeaderMap::new();
    headers.insert("location", http::HeaderValue::from_static("/accounts/42"));
    headers.insert("etag", http::HeaderValue::from_static("\"v1\""));
    headers.insert("cache-control", http::HeaderValue::from_static("no-store"));
    // Multi-valued header: both Set-Cookie lines must round-trip.
    headers.append("set-cookie", http::HeaderValue::from_static("a=1"));
    headers.append("set-cookie", http::HeaderValue::from_static("b=2"));
    // Filtered headers — must NOT appear after round trip.
    headers.insert(
        "date",
        http::HeaderValue::from_static("Mon, 01 Jan 2024 00:00:00 GMT"),
    );
    headers.insert("content-length", http::HeaderValue::from_static("42"));
    headers.insert("connection", http::HeaderValue::from_static("close"));
    headers.insert(
        "transfer-encoding",
        http::HeaderValue::from_static("chunked"),
    );

    let blob = encode_headers(&headers);
    let restored = decode_headers(&blob);

    assert_eq!(
        restored.get("location").unwrap().as_bytes(),
        b"/accounts/42"
    );
    assert_eq!(restored.get("etag").unwrap().as_bytes(), b"\"v1\"");
    assert_eq!(
        restored.get("cache-control").unwrap().as_bytes(),
        b"no-store"
    );
    let cookies: Vec<_> = restored.get_all("set-cookie").iter().collect();
    assert_eq!(cookies.len(), 2, "multi-valued Set-Cookie must round-trip");

    assert!(restored.get("date").is_none(), "Date is filtered");
    assert!(
        restored.get("content-length").is_none(),
        "Content-Length is recomputed by the framework",
    );
    assert!(restored.get("connection").is_none(), "hop-by-hop");
    assert!(restored.get("transfer-encoding").is_none(), "hop-by-hop");
}

#[test]
fn decode_headers_of_empty_blob_returns_empty_map() {
    let map = decode_headers(&[]);
    assert!(map.is_empty());
}

#[test]
fn decode_headers_tolerates_truncated_blob_without_panicking() {
    // The middleware treats a corrupt headers blob as a recoverable
    // curiosity — the replay still returns the right status and
    // body. If a future change made `decode_headers` panic on
    // partial input, a single corrupted row would crash every
    // replay against that key.
    let truncated = [42u8, 0, 0, 0, 5, 0, 0, 0, b'x']; // claims 42 entries, 5-byte name, only 1 byte present
    let map = decode_headers(&truncated);
    assert!(map.is_empty());
}
