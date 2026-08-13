use std::net::IpAddr;

use ipnet::IpNet;

/// Which single forwarding header a trusted proxy is expected to write.
///
/// RFC 7239 `Forwarded` and the legacy `X-Forwarded-For` are alternatives,
/// not complements — a real reverse proxy (nginx, an AWS ALB, HAProxy's
/// defaults) is configured to emit **one** of the two, never both
/// meaningfully. Trusting whichever one happens to be present on the wire
/// is exactly the bypass this type exists to close: an attacker who knows
/// a deployment trusts `X-Forwarded-For` can simply add a `Forwarded`
/// header instead — until this field existed, that header was honored
/// unconditionally over `X-Forwarded-For` with no hop-count or
/// trusted-peer check ever applied to it (#415 remediation). Naming the
/// header explicitly, defaulting to the header real proxies actually send,
/// closes that gap: the rarer header only takes effect when a deployment
/// opts into it because its own proxy is actually configured to emit it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ForwardedHeader {
    /// The legacy header, still what the overwhelming majority of real
    /// deployments emit (nginx's `proxy_set_header X-Forwarded-For`, AWS
    /// ALB, HAProxy's defaults). The safe default.
    #[default]
    XForwardedFor,
    /// RFC 7239 `Forwarded`. Select this only when the deployment's own
    /// trusted proxy is actually configured to emit `Forwarded` instead of
    /// `X-Forwarded-For` — most are not.
    Forwarded,
}

/// Which peers are trusted to set `Forwarded`/`X-Forwarded-For`, how many
/// hops into the chain to trust when they are, and which of the two
/// headers to honor.
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
    header: ForwardedHeader,
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
    /// configured here). The forwarding header defaults to
    /// [`ForwardedHeader::XForwardedFor`] — call [`Self::forwarded_header`]
    /// if the deployment's proxy actually emits RFC 7239 `Forwarded`
    /// instead.
    ///
    /// A bare host address can be supplied via `IpAddr`'s `Into<IpNet>`
    /// impl (a full-length /32 or /128 prefix): `IpAddr::from(...).into()`.
    pub fn trusting(allowlist: impl IntoIterator<Item = IpNet>) -> Self {
        Self {
            allowlist: allowlist.into_iter().collect(),
            max_hops: 1,
            header: ForwardedHeader::default(),
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

    /// Select which single header this deployment's trusted proxy writes.
    /// See [`ForwardedHeader`]'s doc for why only one is ever honored.
    pub fn forwarded_header(mut self, header: ForwardedHeader) -> Self {
        self.header = header;
        self
    }

    pub(crate) fn hop_count(&self) -> usize {
        self.max_hops
    }

    pub(crate) fn header(&self) -> ForwardedHeader {
        self.header
    }

    /// Whether `peer` — the verified socket peer address, never a
    /// client-suppliable value — is a configured trusted proxy.
    pub fn is_trusted(&self, peer: IpAddr) -> bool {
        self.allowlist.iter().any(|net| net.contains(&peer))
    }
}
