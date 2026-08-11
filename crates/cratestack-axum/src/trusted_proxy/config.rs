use std::net::IpAddr;

use ipnet::IpNet;

/// Which peers are trusted to set `Forwarded`/`X-Forwarded-For`, and how
/// many hops into the chain to trust when they are.
///
/// Applied by the consumer as a plain `Extension<TrustedProxyConfig>`
/// (`.layer(Extension(config))`) on every router the app serves —
/// resolved inline inside [`crate::headers::enrich_context_from_headers`]
/// rather than through a bespoke `tower::Layer`/`Service` pair (Option A',
/// `docs/design/trusted-proxy-client-ip.md`).
///
/// The default ([`TrustedProxyConfig::none`], and the behavior when no
/// `Extension` is applied at all) trusts nothing: `Forwarded`/
/// `X-Forwarded-For` are never honored. `enrich_context_from_headers`
/// falls back to the verified socket peer address (via
/// `axum::extract::ConnectInfo<SocketAddr>`) when one is available, or
/// records no `client_ip` at all when it isn't — never guessing, never
/// trusting an unverified header. See decision 3 in the design doc.
#[derive(Clone, Debug, Default)]
pub struct TrustedProxyConfig {
    allowlist: Vec<IpNet>,
    max_hops: usize,
}

impl TrustedProxyConfig {
    /// Trust nothing. Equivalent to omitting the `Extension` entirely —
    /// provided as an explicit, self-documenting constructor for callers
    /// who want to state the choice rather than rely on absence.
    pub fn none() -> Self {
        Self::default()
    }

    /// Trust the given peers (exact host addresses or CIDR ranges) as
    /// reverse proxies. `max_hops` defaults to `1` (a single trusted
    /// proxy) — call [`Self::max_hops`] to widen it for a chain of
    /// several trusted proxies (e.g. CDN + load balancer, both
    /// configured here).
    ///
    /// A bare host address can be supplied via `IpAddr`'s `Into<IpNet>`
    /// impl (a full-length /32 or /128 prefix): `IpAddr::from(...).into()`.
    pub fn trusting(allowlist: impl IntoIterator<Item = IpNet>) -> Self {
        Self {
            allowlist: allowlist.into_iter().collect(),
            max_hops: 1,
        }
    }

    /// How many entries, counted from the right (proxy) end of the
    /// `Forwarded`/`X-Forwarded-For` chain, are trusted to have been
    /// appended by a trusted proxy.
    ///
    /// This **must** be interpreted right-to-left: the entry taken is the
    /// one `max_hops` positions in from the right end, not the
    /// `max_hops`-th entry from the left. The left end of the chain is
    /// exactly the part an untrusted client controls — walking from the
    /// left re-opens the spoofing gap this type exists to close for any
    /// chain with more than one hop. See decision 5 in
    /// `docs/design/trusted-proxy-client-ip.md`.
    pub fn max_hops(mut self, max_hops: usize) -> Self {
        self.max_hops = max_hops;
        self
    }

    pub(crate) fn hop_count(&self) -> usize {
        self.max_hops
    }

    /// Whether `peer` — the verified socket peer address, never a
    /// client-suppliable value — is a configured trusted proxy.
    pub fn is_trusted(&self, peer: IpAddr) -> bool {
        self.allowlist.iter().any(|net| net.contains(&peer))
    }
}
