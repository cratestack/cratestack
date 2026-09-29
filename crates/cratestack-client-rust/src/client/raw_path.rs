//! Which raw bridge paths a sealing client can bind (cratestack#1007).

/// The op id of a raw `/rpc/{op}` path, or `None` for anything else.
///
/// The server's `is_op_id` (a dotted id without `/`, and never `batch`) plus
/// `batch` itself, and nothing that could re-route the request: no query,
/// fragment, escape, backslash or dot segment.
pub(crate) fn rpc_op(path: &str) -> Option<&str> {
    path.strip_prefix("/rpc/").filter(|op| {
        (*op == "batch" || op.contains('.'))
            && !op.contains(['/', '?', '#', '%', '\\'])
            && !op.split('.').any(str::is_empty)
    })
}

#[cfg(all(test, feature = "cose"))]
mod tests {
    use super::rpc_op;

    #[test]
    fn op_ids_and_batch_are_accepted() {
        assert_eq!(rpc_op("/rpc/model.User.list"), Some("model.User.list"));
        assert_eq!(rpc_op("/rpc/procedure.ping"), Some("procedure.ping"));
        assert_eq!(rpc_op("/rpc/batch"), Some("batch"));
    }

    #[test]
    fn anything_that_could_reroute_is_refused() {
        for path in [
            "/rpc/",
            "/rpc/ping",
            "/rpc/a.b?x=1",
            "/rpc/a.b#f",
            "/rpc/a.b%2f",
            "/rpc/a\\b.c",
            "/rpc/a/b.c",
            "/rpc/..",
            "/rpc/a..b",
            "/rpc/.a",
            "/rpc/a.",
            "/api/a.b",
        ] {
            assert_eq!(rpc_op(path), None, "{path}");
        }
    }
}
