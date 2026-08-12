//! Informational timing measurement (not a correctness test, not run by
//! default — see `#[ignore]` below) for the unconditional
//! `extensions.clone()` `ClientIpContext::from_extensions` now performs
//! on every request, on every transport, per the review that asked this
//! cost be measured rather than left silent or hand-waved as "probably
//! fine".
//!
//! Builds an `http::Extensions` matching what this framework's own
//! middleware actually inserts on a served router — `ConnectInfo
//! <SocketAddr>` (via `into_make_service_with_connect_info`) and a
//! `TrustedProxyConfig` with a realistic 3-entry CIDR allowlist (via
//! `.layer(Extension(TrustedProxyConfig::trusting(...)))`, #415) — and
//! times `.clone()` against it in a tight loop, alongside the same
//! measurement for `http::HeaderMap` with a handful of headers (a
//! representative real request), since that clone already runs
//! unconditionally on every request today (`axum-core-0.5.6`'s own
//! `HeaderMap: FromRequestParts` impl) and is the fairest baseline for
//! "is the new cost the same class, not a new one".
//!
//! Run explicitly: `cargo test -p cratestack-axum -- --ignored --nocapture
//! extensions_clone_cost`. The pasted numbers from an actual run live in
//! `ClientIpContext`'s doc comment and this ticket's PR description —
//! re-run this yourself rather than trusting last year's numbers if this
//! ever becomes a real question again.

#![cfg(test)]

use std::net::SocketAddr;
use std::time::Instant;

use axum::extract::ConnectInfo;
use axum::http::{Extensions, HeaderMap, HeaderValue};

use crate::trusted_proxy::TrustedProxyConfig;

fn realistic_extensions() -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(ConnectInfo::<SocketAddr>(
        "203.0.113.9:443".parse().unwrap(),
    ));
    extensions.insert(
        TrustedProxyConfig::trusting([
            "10.0.0.0/8".parse().unwrap(),
            "192.168.0.0/16".parse().unwrap(),
            "198.51.100.1/32".parse().unwrap(),
        ])
        .max_hops(2),
    );
    extensions
}

fn realistic_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/cbor"));
    headers.insert("accept", HeaderValue::from_static("application/cbor"));
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer eyJhbGciOiJIUzI1NiJ9.abcdef.ghijkl"),
    );
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
    );
    headers
}

#[test]
#[ignore = "informational timing measurement, not a correctness assertion \
            (would be flaky as a CI gate) — run explicitly with \
            `cargo test -p cratestack-axum -- --ignored --nocapture \
            extensions_clone_cost`"]
fn extensions_clone_cost_vs_headermap_clone_cost() {
    const ITERS: u32 = 200_000;

    let extensions = realistic_extensions();
    let started = Instant::now();
    for _ in 0..ITERS {
        let cloned = extensions.clone();
        std::hint::black_box(&cloned);
    }
    let extensions_elapsed = started.elapsed();

    let headers = realistic_headers();
    let started = Instant::now();
    for _ in 0..ITERS {
        let cloned = headers.clone();
        std::hint::black_box(&cloned);
    }
    let headers_elapsed = started.elapsed();

    eprintln!(
        "Extensions::clone(): {:?}/iter over {ITERS} iters (total {:?})",
        extensions_elapsed / ITERS,
        extensions_elapsed,
    );
    eprintln!(
        "HeaderMap::clone():  {:?}/iter over {ITERS} iters (total {:?})",
        headers_elapsed / ITERS,
        headers_elapsed,
    );
}
