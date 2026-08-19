//! Cross-cutting request/response size ceilings for the generated Axum
//! surface (cratestack#413). Lives in its own module rather than being
//! appended to `page.rs`/`batch.rs`: those two already own their own
//! numeric ceiling (`MAX_LIST_LIMIT`, `BATCH_MAX_ITEMS`) scoped to their
//! own concern, so a body/response-size constant that cuts across both
//! REST and RPC belongs in a module of its own — see
//! `docs/design/request-response-size-bounds.md` (Reviewer notes) for the
//! reasoning.
//!
//! Both constants below are threaded through the generated `router()` /
//! `rpc_router()` entry points as an explicit `body_limit_bytes: usize`
//! parameter, not applied as a fixed `.layer(...)` a consumer is expected
//! to re-layer on top of. That's a deliberate, empirically-verified
//! choice, not an oversight: `axum::extract::DefaultBodyLimit` is
//! extension-based, and `DefaultBodyLimitService::call` unconditionally
//! overwrites that extension on every invocation with no "already set"
//! check. Because a consumer can only ever wrap a `Router` cratestack has
//! already returned — never insert a layer *between* cratestack's own
//! layer and the handler — whichever `DefaultBodyLimit` sits closest to
//! the handler always wins, and that's structurally always cratestack's,
//! regardless of whether the consumer's re-layered value is larger,
//! smaller, or a `disable()`. See `docs/design/request-response-size-bounds.md`
//! Decision 2 for the reproduction. A real constructor parameter has no
//! such failure mode: exactly one `DefaultBodyLimit` layer is ever
//! constructed, with the caller's chosen value baked in once.

/// Default request body limit (bytes) for the generated `router()` /
/// `rpc_router()` entry points, applied via
/// `axum::extract::DefaultBodyLimit::max(body_limit_bytes)`.
///
/// Set to **2 MiB specifically because that's axum's own implicit
/// default already** — `axum::body::Bytes`'s `FromRequest` impl (and
/// everything built on it: `String`, `Json`, `Form`) refuses bodies over
/// 2 MiB out of the box, with no layer required at all, verified against
/// the vendored `axum-core 0.5.6` this workspace's `Cargo.lock` actually
/// pins (its own doc comment: "For security reasons, `Bytes` will, by
/// default, not accept bodies larger than 2MB"). Every generated handler
/// extracts `Bytes`, so request bodies were *already* implicitly capped
/// at 2 MiB before this constant existed — the gap #413 closes is that
/// this limit was invisible, undocumented, and not expressed anywhere in
/// cratestack's own code, not that a limit was absent.
///
/// Matching that number rather than picking a smaller, "more considered"
/// one makes this constant — and the `DefaultBodyLimit` layer built from
/// it — **provably a no-op on upgrade**: naming, documenting, and making
/// overridable a limit that was already there, instead of silently
/// tightening what an existing deployment can send. An earlier revision
/// of this constant used 1 MiB; that was reverted once
/// `docs/design/request-response-size-bounds.md`'s Decision 2 (written
/// independently, without knowledge of that choice) made the "match
/// axum's own default" argument explicit — see that section for the
/// full reasoning and the empirical check that `BATCH_MAX_ITEMS` (1000
/// frames) still fits comfortably inside a 2 MiB request at any
/// realistic average frame size.
///
/// A deployment that legitimately needs to accept larger bodies passes a
/// bigger value to `router(..., body_limit_bytes)` / `rpc_router(...,
/// body_limit_bytes)` — every generated entry point takes this as an
/// explicit, real parameter (see this module's doc comment for why that,
/// and not re-layering, is the supported override mechanism).
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Bound used at every `axum::body::to_bytes(body, N)` call site that
/// re-buffers a `Response` produced in-process (RPC batch per-frame
/// re-encoding, handler-error re-shaping, and the per-frame codec
/// round-trip helper — see
/// `crates/cratestack-axum/src/rpc/{batch,error_encode,
/// codec_helpers}.rs`). None of these three sites face an untrusted
/// upstream/proxied body; all buffer a response cratestack itself
/// produced, so this is a safety valve against a pathological in-process
/// response (e.g. a handler bug or a legitimately huge result set),
/// never a network-trust boundary the way [`DEFAULT_BODY_LIMIT_BYTES`]
/// is.
///
/// Set to 4× [`DEFAULT_BODY_LIMIT_BYTES`] rather than reusing it
/// outright: a `create`/`update` response that echoes the request
/// payload plus server-added columns could legitimately land right at
/// the request ceiling, so reusing the request-side number risks
/// spurious failures on the response side. The 4× multiplier isn't a
/// bare guess either — it has an actual ceiling-on-the-ceiling to point
/// to: [`crate::page::MAX_LIST_LIMIT`] (1000 rows) already bounds how
/// large a `list` response's item array can be, and
/// [`crate::batch::BATCH_MAX_ITEMS`]-capped batch responses are
/// similarly row-count-bounded, so a response several times the request
/// ceiling comfortably covers realistic multi-row payloads without
/// removing the ceiling altogether.
///
/// Exceeding this bound does not panic: every call site already matches
/// on `to_bytes`'s `Result` and degrades to a synthesized
/// `CratestackError::Internal` / error frame on `Err`, which is exactly what a
/// `LengthLimitError` produces once the body is capped instead of
/// unbounded.
pub const MAX_RESPONSE_REBUFFER_BYTES: usize = 4 * DEFAULT_BODY_LIMIT_BYTES;

// Compile-time invariant, not a runtime test: `DEFAULT_BODY_LIMIT_BYTES`
// must never exceed the idempotency middleware's own request-buffering
// cap (`MAX_BODY_BYTES`) without that being a deliberate, separately
// reviewed decision — see the "Coherence note" in
// `docs/design/request-response-size-bounds.md` for why the two are now
// numerically *equal* (both 2 MiB) rather than the router being strictly
// tighter, and why that's still safe today. `<=`, not `<`: at 2 MiB the
// two legitimately coincide, so a strict inequality would fail to build
// the moment this constant reused the same number as `MAX_BODY_BYTES`
// on purpose, which is exactly what happened here.
const _: () = assert!(DEFAULT_BODY_LIMIT_BYTES <= crate::store::idempotency::MAX_BODY_BYTES);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_body_limit_matches_axums_own_implicit_bytes_default() {
        assert_eq!(DEFAULT_BODY_LIMIT_BYTES, 2 * 1024 * 1024);
    }

    #[test]
    fn response_rebuffer_bound_is_four_times_the_request_default() {
        assert_eq!(MAX_RESPONSE_REBUFFER_BYTES, 8 * 1024 * 1024);
    }
}
