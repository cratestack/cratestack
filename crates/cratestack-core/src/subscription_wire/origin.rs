//! Which browser origins may open a session (design §6.3, "Origin").
//!
//! `Origin` is a browser protection: a page on another site must not be
//! able to ride a user's ambient credentials into a session. A request
//! without `Origin` is not from a browser page and is not subject to it —
//! native clients authenticate explicitly.

/// Which `Origin`s are accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginPolicy {
    /// Only pages served from the session's own scheme, host and port.
    SameOrigin,
    /// Exactly these origins, written as `scheme://host[:port]`.
    Allow(Vec<String>),
}

impl OriginPolicy {
    /// `authority` is the CONNECT request's `host[:port]`.
    pub fn permits(&self, origin: Option<&str>, authority: &str) -> bool {
        let Some(origin) = origin else {
            return true;
        };
        let Some(origin) = normalize(origin) else {
            return false;
        };
        match self {
            Self::SameOrigin => normalize(&format!("https://{authority}")) == Some(origin),
            Self::Allow(allowed) => allowed
                .iter()
                .any(|a| normalize(a).as_deref() == Some(&origin)),
        }
    }
}

impl OriginPolicy {
    /// [`Self::permits`] for a handshake that may arrive over plain HTTP: a
    /// WebSocket upgrade behind a TLS-terminating proxy, or in development,
    /// cannot tell which scheme the page used, so `SameOrigin` accepts the
    /// authority's `http` origin as well as its `https` one.
    pub fn permits_any_scheme(&self, origin: Option<&str>, authority: &str) -> bool {
        self.permits(origin, authority)
            || (*self == Self::SameOrigin
                && origin.and_then(normalize) == normalize(&format!("http://{authority}")))
    }
}

/// `scheme://host:port`, lowercased, default port made explicit; `None`
/// for anything that is not an http(s) origin (including the literal
/// `null` an opaque origin sends).
fn normalize(origin: &str) -> Option<String> {
    let origin = origin.trim().to_ascii_lowercase();
    let (scheme, rest) = origin.split_once("://")?;
    let default_port = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    let host_port = rest.trim_end_matches('/');
    if host_port.is_empty() || host_port.contains('/') {
        return None;
    }
    // A trailing `:digits` is a port unless it is part of an unbracketed
    // IPv6 literal; a bracketed one (`[::1]:4433`) keeps its brackets.
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit())
                && (!host.contains(':') || host.ends_with(']')) =>
        {
            (host, port.parse::<u16>().ok()?)
        }
        _ => (host_port, default_port),
    };
    Some(format!("{scheme}://{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_http_handshake_accepts_the_http_origin_too() {
        let policy = OriginPolicy::SameOrigin;
        assert!(policy.permits_any_scheme(Some("http://localhost:3000"), "localhost:3000"));
        assert!(policy.permits_any_scheme(Some("https://localhost:3000"), "localhost:3000"));
        assert!(!policy.permits_any_scheme(Some("http://evil.example"), "localhost:3000"));
        assert!(!policy.permits(Some("http://localhost:3000"), "localhost:3000"));
    }

    #[test]
    fn no_origin_header_means_not_a_browser_page() {
        assert!(OriginPolicy::SameOrigin.permits(None, "api.example.com"));
    }

    #[test]
    fn same_origin_compares_scheme_host_and_effective_port() {
        let policy = OriginPolicy::SameOrigin;
        assert!(policy.permits(Some("https://api.example.com"), "api.example.com"));
        assert!(policy.permits(Some("https://API.example.com:443"), "api.example.com:443"));
        assert!(!policy.permits(Some("https://evil.example"), "api.example.com"));
        assert!(!policy.permits(Some("http://api.example.com"), "api.example.com"));
        assert!(!policy.permits(Some("https://api.example.com:8443"), "api.example.com"));
        assert!(!policy.permits(Some("null"), "api.example.com"));
    }

    #[test]
    fn allow_list_matches_exact_origins_only() {
        let policy = OriginPolicy::Allow(vec![
            "https://app.example.com".into(),
            "http://localhost:5173".into(),
        ]);
        assert!(policy.permits(Some("https://app.example.com"), "api.example.com"));
        assert!(policy.permits(Some("http://localhost:5173"), "127.0.0.1:4433"));
        assert!(!policy.permits(Some("http://localhost:5174"), "127.0.0.1:4433"));
        assert!(!policy.permits(Some("https://app.example.com.evil"), "api.example.com"));
    }

    #[test]
    fn ipv6_origins_keep_their_brackets() {
        let policy = OriginPolicy::Allow(vec!["https://[::1]:4433".into()]);
        assert!(policy.permits(Some("https://[::1]:4433"), "x"));
        assert!(!policy.permits(Some("https://[::1]"), "x"));
    }
}
