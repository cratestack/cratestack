//! What a transport does when the rate-limit *store* itself fails
//! (cratestack#846), and how long it waits to find out.
//!
//! Moved here from `cratestack-axum`'s `ratelimit/policy.rs` by the
//! cratestack#1038 maintainer decision (2026-09-24): MCP hard-coded HTTP's
//! default instead of sharing the type, so an application that chose
//! [`StoreErrorPolicy::Deny`] on HTTP was fail-open over MCP. With the type
//! at L3, both transports take the same value and apply the same
//! [`StoreErrorPolicy::permits`] rule. `cratestack-axum` re-exports both
//! items from their old path, so `cratestack_axum::ratelimit::StoreErrorPolicy`
//! still names this exact type.
//!
//! Only the *decision* lives here. Enforcing [`DEFAULT_STORE_TIMEOUT`]
//! needs a timer, and logging and rendering the outcome are the
//! transport's; none of that can move without giving this crate a runtime
//! or a `tracing` edge, which ADR 0015 excludes (the crate depends on
//! `cratestack-core` only).

use std::time::Duration;

use cratestack_core::CratestackError;

/// How a transport's rate limiter treats a failure of the backing
/// [`cratestack_core::RateLimitStore`], as distinct from a caller who is
/// genuinely over budget.
///
/// # The distinction that matters is transport vs logical, not open vs closed
///
/// The first cut of this knob was "on any store error, allow". A security
/// review falsified the premise it rested on — that a store failure is
/// never caller-controlled — with a measured attack: `cratestack-axum`'s
/// default key function hashes an **unvalidated** `Authorization` header
/// (its layer runs before authentication), so an unauthenticated caller
/// mints one Redis key per request just by rotating that header. Drive
/// that until the instance reaches `maxmemory` and every subsequent
/// `HSET` fails with `OOM` — at which point a blanket fail-open serves
/// *every* request unthrottled, including from buckets that were already
/// exhausted. The bypass is reachable by anyone.
///
/// So the axis is not "open vs closed". It is:
///
/// - A **transport** failure — the socket broke, the server is
///   unreachable — is not caller-controlled and self-heals once the
///   connection is replaced. Refusing here converts a limiter hiccup into
///   a simultaneous outage of every rate-limited route, for a condition
///   nobody in the request path can fix. This is what
///   [`StoreErrorPolicy::Allow`] serves through.
/// - A **logical** failure — the store was reached and said no (`OOM`, a
///   permission error, a poisoned mutex, a malformed reply) — may be
///   caller-induced, does not self-heal, and is exactly the shape an
///   attacker steers toward. It stays closed under **every** policy.
///
/// Concretely: `Allow` matches [`CratestackError::Unavailable`] and nothing
/// else. Backends signal transport-class failures with that variant
/// (`cratestack-redis`'s `ratelimit::util::is_transport_class`); anything
/// else they return is refused even under `Allow`.
///
/// Key derivation remains fail-closed under both policies (cratestack#416)
/// for the same reason the OOM case is: its inputs are caller-controlled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StoreErrorPolicy {
    /// Serve the request unthrottled when — and only when — the store
    /// failure is transport-class. Every other store failure is refused
    /// exactly as under [`StoreErrorPolicy::Deny`].
    #[default]
    Allow,
    /// Refuse on any store failure, transport-class included, with the
    /// store's own error and the transport's normal typed error.
    /// For deployments where the limiter is a security control (a
    /// paywall, a brute-force guard) rather than a capacity control.
    Deny,
}

impl StoreErrorPolicy {
    /// Whether this policy serves `error` through to the operation.
    ///
    /// `pub` rather than the `pub(super)` it was in `cratestack-axum`,
    /// because it is the rule the two transports now share: a transport
    /// that re-derived it (as MCP once did, with a hand-written
    /// `Unavailable` match) could drift from HTTP without either side's
    /// tests noticing.
    ///
    /// Deliberately a match on the error *variant* rather than on
    /// `status_code()`: 503 is also reachable from a hand-written store
    /// that means something else by it, and a helper whose job is to gate
    /// a security-relevant bypass should be readable without a detour
    /// through the HTTP mapping table.
    pub fn permits(self, error: &CratestackError) -> bool {
        match self {
            Self::Deny => false,
            Self::Allow => matches!(error, CratestackError::Unavailable(_)),
        }
    }
}

/// Default ceiling on one `store.consume` call — first attempt *and* any
/// backend-internal retry, as a single budget.
///
/// The security review measured the alternative: `redis`'s
/// `ConnectionManager` defaults both its connection and response timeouts
/// to `None`, so during a real outage each attempt awaited an unbounded
/// reconnect cycle — 9.46s, doubled to 18.92s by the retry. "Degrade to
/// unlimited" silently meant "hang for nineteen seconds, then allow",
/// which is worse for the caller than the refusal it replaced and is
/// itself a denial-of-service lever.
///
/// 500ms is chosen to be far above a healthy Redis round-trip (sub-
/// millisecond on a local network, single-digit milliseconds across an
/// availability zone) and far below anything a human would call a hang.
/// An elapsed budget is a transport-class failure — the store did not
/// answer — so a transport reports it as [`CratestackError::Unavailable`]
/// and applies [`StoreErrorPolicy::permits`] to it like any other.
///
/// It lives beside the policy so HTTP and MCP cannot drift apart on the
/// default; each transport enforces it with its own timer.
/// `cratestack-axum`'s `RateLimitLayer::with_store_timeout` tunes it on
/// HTTP.
pub const DEFAULT_STORE_TIMEOUT: Duration = Duration::from_millis(500);
