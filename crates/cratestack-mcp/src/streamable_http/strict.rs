//! Request shapes the guard refuses although `rmcp` would serve them
//! (found in review of cratestack#1039).
//!
//! **A mirrored MCP header sent twice.** `rmcp` checks `MCP-Protocol-Version`,
//! `Mcp-Method`, `Mcp-Name` and `Mcp-Param-*` against the body, but only
//! the first value of each. An intermediary that routes or rate-limits on
//! the header may read another value, which is the split between "sources
//! of truth" the spec's header validation exists to prevent. A conforming
//! client sends each at most once.

use http::{HeaderMap, HeaderName};

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
    use http::HeaderMap;

    use super::repeated_mirror;

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
