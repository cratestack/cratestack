//! The route a call binds into its COSE envelope (cratestack#1007).
//!
//! A sealed request binds the route *template* (`/widgets/{id}`), not the
//! URL it was sent to, plus the values the template's parameters took: a
//! gateway may rewrite the path, and the template alone would not tell the
//! answer for `/widgets/1` from the answer for `/widgets/2`. The client
//! builds concrete paths, so the generated code says which template a call
//! is for. For RPC the route is the op id.

/// The route template of one call, and the values of its `{parameters}` in
/// template order.
///
/// The template is the schema's own path string, the same one the server
/// lists in `ROUTE_TRANSPORTS`, without the mount prefix; for an RPC call it
/// is the op id (`model.Widget.get`, `procedure.ping`) or `batch`. Without
/// the `cose` feature, or without an envelope on the client, a `RouteRef`
/// is ignored.
///
/// ```
/// use cratestack_client_rust::RouteRef;
///
/// let id = 7.to_string();
/// let params = [id.as_str()];
/// let route = RouteRef::new("/widgets/{id}", &params);
/// assert_eq!(route.template(), "/widgets/{id}");
/// assert_eq!(route.params(), ["7"]);
/// assert_eq!(RouteRef::rpc("model.Widget.list").params().len(), 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRef<'a> {
    template: &'a str,
    params: &'a [&'a str],
}

impl<'a> RouteRef<'a> {
    /// A REST route: its template and the decoded values of its parameters,
    /// in template order.
    pub const fn new(template: &'a str, params: &'a [&'a str]) -> Self {
        Self { template, params }
    }

    /// An RPC route: the op id, or `"batch"`. RPC has no path parameters.
    pub const fn rpc(op_id: &'a str) -> Self {
        Self {
            template: op_id,
            params: &[],
        }
    }

    pub const fn template(&self) -> &'a str {
        self.template
    }

    pub const fn params(&self) -> &'a [&'a str] {
        self.params
    }
}

/// `segment` as one URL path segment: everything but the RFC 3986 unreserved
/// characters is percent-encoded, so an id such as `50%off` or `a/b` reaches
/// the server as the value the seal binds.
///
/// ```
/// use cratestack_client_rust::encode_path_segment;
///
/// assert_eq!(encode_path_segment("abc-1.2_3~"), "abc-1.2_3~");
/// assert_eq!(encode_path_segment("50%off"), "50%25off");
/// assert_eq!(encode_path_segment("a/b c"), "a%2Fb%20c");
/// ```
pub fn encode_path_segment(segment: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
