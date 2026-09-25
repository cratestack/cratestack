//! Two request shapes the guard refuses although `rmcp` would serve them
//! (found in review of cratestack#1039).
//!
//! **A token in the query string.** MCP's authorization spec forbids it and
//! OAuth 2.1 dropped the query method. The guard cannot know whether the
//! application's provider reads `access_token` from the query (a REST
//! provider shared with SSE or websocket routes often does), and it strips
//! only `Authorization`, so such a token would be accepted and then travel
//! on in the request URI to `rmcp` and the handler. Refusing it before the
//! provider runs keeps "the token goes nowhere" true whatever the provider
//! does. The endpoint defines no query parameters, so nothing legitimate is
//! lost.
//!
//! **A mirrored MCP header sent twice.** `rmcp` checks `MCP-Protocol-Version`,
//! `Mcp-Method`, `Mcp-Name` and `Mcp-Param-*` against the body, but only
//! the first value of each. An intermediary that routes or rate-limits on
//! the header may read another value, which is the split between "sources
//! of truth" the spec's header validation exists to prevent. A conforming
//! client sends each at most once.

use http::{HeaderMap, HeaderName, Uri};

/// RFC 6750 §2.3's parameter name.
const QUERY_TOKEN: &str = "access_token";

/// `true` when the query carries an `access_token` parameter, however its
/// name is percent-encoded.
pub(crate) fn query_token(uri: &Uri) -> bool {
    uri.query().is_some_and(|query| {
        serde_urlencoded::from_str::<Vec<(String, String)>>(query)
            // Decoding is lossy and does not fail in practice; if it ever
            // did, an unreadable query is refused rather than trusted.
            .map_or(true, |pairs| {
                pairs.iter().any(|(name, _)| name == QUERY_TOKEN)
            })
    })
}

/// The first header the body mirrors that appears more than once.
pub(crate) fn repeated_mirror(headers: &HeaderMap) -> Option<&HeaderName> {
    headers.keys().find(|name| {
        let mirrored = matches!(
            name.as_str(),
            "mcp-protocol-version" | "mcp-method" | "mcp-name"
        ) || name.as_str().starts_with("mcp-param-");
        mirrored && headers.get_all(*name).iter().nth(1).is_some()
    })
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, Uri};

    use super::{query_token, repeated_mirror};

    #[test]
    fn a_query_token_is_found_however_it_is_spelled() {
        for uri in [
            "/mcp?access_token=t",
            "/mcp?a=1&access_token=",
            "/mcp?access%5Ftoken=t",
            "/mcp?access_token",
        ] {
            assert!(query_token(&uri.parse::<Uri>().unwrap()), "{uri}");
        }
        for uri in ["/mcp", "/mcp?", "/mcp?token=t", "/mcp?x=access_token"] {
            assert!(!query_token(&uri.parse::<Uri>().unwrap()), "{uri}");
        }
    }

    #[test]
    fn only_a_repeated_mirrored_header_counts() {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("mcp-method", "tools/call"),
            ("mcp-name", "echo"),
            ("mcp-session-id", "a"),
            ("mcp-session-id", "b"),
            ("accept", "application/json"),
            ("accept", "text/event-stream"),
        ] {
            headers.append(name, value.parse().unwrap());
        }
        assert_eq!(repeated_mirror(&headers), None);

        headers.append("Mcp-Param-Region", "a".parse().unwrap());
        headers.append("mcp-param-region", "b".parse().unwrap());
        assert_eq!(repeated_mirror(&headers).unwrap(), "mcp-param-region");
    }
}
