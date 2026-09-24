//! The `Origin` allowlist (MCP 2026-07-28 Streamable HTTP: "servers MUST
//! validate the `Origin` header"; a present, invalid one gets 403).
//!
//! **Why this crate checks it itself** when `rmcp` has a check too: `rmcp`
//! turns its check off when the list is empty, and that default is the
//! reason a browser page on another origin could drive a local server (DNS
//! rebinding). Both checks run, and both are required to pass. The builder
//! also sets `rmcp`'s `validate_empty_origin_allowlist`, so neither check
//! alone can be switched off by an empty list.
//!
//! **Matching** is RFC 6454's origin tuple: scheme, host (case-insensitive)
//! and port, with a missing port read as the scheme's default. So
//! `https://app.example.com` allows `https://app.example.com:443` and
//! nothing else. That is stricter than `rmcp`, where an entry without a port
//! allows every port. The literal `null` (sandboxed frames, `file:` pages)
//! matches only when it is listed. A request with no `Origin` passes:
//! non-browser clients do not send one, and the check exists for browsers.

use http::HeaderMap;
use http::header::ORIGIN;

use super::error::HttpConfigError;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    Null,
    Tuple {
        scheme: String,
        host: String,
        port: Option<u16>,
    },
}

fn parse(value: &str) -> Option<Origin> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("null") {
        return Some(Origin::Null);
    }
    let uri = http::Uri::try_from(value).ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let authority = uri.authority()?;
    // An origin is scheme, host and port: no user info, path or query.
    let bare = !authority.as_str().contains('@')
        && uri.query().is_none()
        && matches!(uri.path(), "" | "/")
        && !value.ends_with('/');
    if !bare || authority.host().is_empty() {
        return None;
    }
    let port = authority.port_u16().or(match scheme.as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    });
    Some(Origin::Tuple {
        scheme,
        host: authority.host().to_ascii_lowercase(),
        port,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct AllowedOrigins {
    parsed: Vec<Origin>,
    /// The entries as given, for `rmcp`'s own copy of the check.
    raw: Vec<String>,
}

impl AllowedOrigins {
    pub(crate) fn new(raw: Vec<String>) -> Result<Self, HttpConfigError> {
        if raw.is_empty() {
            return Err(HttpConfigError::NoAllowedOrigins);
        }
        let parsed = raw
            .iter()
            .map(|entry| parse(entry).ok_or_else(|| HttpConfigError::InvalidOrigin(entry.clone())))
            .collect::<Result<_, _>>()?;
        Ok(Self { parsed, raw })
    }

    pub(crate) fn raw(&self) -> &[String] {
        &self.raw
    }

    /// `true` when the request may proceed: no `Origin`, or exactly one
    /// that is on the list. Two `Origin` headers are refused rather than
    /// guessed between.
    pub(crate) fn permits(&self, headers: &HeaderMap) -> bool {
        let mut values = headers.get_all(ORIGIN).iter();
        let Some(value) = values.next() else {
            return true;
        };
        if values.next().is_some() {
            return false;
        }
        value
            .to_str()
            .ok()
            .and_then(parse)
            .is_some_and(|origin| self.parsed.contains(&origin))
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue};

    use super::{AllowedOrigins, HttpConfigError};

    fn allowed(entries: &[&str]) -> AllowedOrigins {
        AllowedOrigins::new(entries.iter().map(|e| (*e).to_owned()).collect()).unwrap()
    }

    fn with_origin(origins: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for origin in origins {
            headers.append("origin", HeaderValue::from_str(origin).unwrap());
        }
        headers
    }

    #[test]
    fn matches_the_origin_tuple_with_default_ports() {
        let list = allowed(&["https://App.Example.com", "http://localhost:8080"]);
        assert!(list.permits(&with_origin(&[])));
        assert!(list.permits(&with_origin(&["https://app.example.com"])));
        assert!(list.permits(&with_origin(&["https://app.example.com:443"])));
        assert!(list.permits(&with_origin(&["http://localhost:8080"])));
        for foreign in [
            "https://app.example.com:8443",
            "http://app.example.com",
            "https://evil.example",
            "http://localhost",
            "null",
            "garbage",
        ] {
            assert!(!list.permits(&with_origin(&[foreign])), "{foreign}");
        }
        let two = with_origin(&["https://app.example.com", "https://evil.example"]);
        assert!(!list.permits(&two), "two Origin headers");
    }

    #[test]
    fn null_matches_only_when_listed() {
        assert!(allowed(&["null"]).permits(&with_origin(&["null"])));
    }

    #[test]
    fn an_empty_or_malformed_list_is_refused() {
        assert_eq!(
            AllowedOrigins::new(Vec::new()).unwrap_err(),
            HttpConfigError::NoAllowedOrigins
        );
        for bad in [
            "*",
            "app.example.com",
            "https://a.example/path",
            "https://u@a.example",
        ] {
            assert_eq!(
                AllowedOrigins::new(vec![bad.to_owned()]).unwrap_err(),
                HttpConfigError::InvalidOrigin(bad.to_owned()),
            );
        }
    }
}
