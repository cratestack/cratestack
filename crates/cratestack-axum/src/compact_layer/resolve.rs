//! Mapping a request to its op in the key tables.

use axum::extract::MatchedPath;
use axum::http::{Method, Request};
use cratestack_core::ContractSelector;
use cratestack_core::compact::{KeyTables, OpKeys};

/// The tables' entry for the op this request calls, at the digest
/// `selector` names. `None` for `/rpc/batch`, an unknown op or digest.
///
/// RPC: a trailing `/rpc/{op_id}`. REST: `"<METHOD> <template>"` for the
/// longest suffix of the matched route that is an op, so a nested mount
/// needs no prefix configuration.
pub(super) fn resolve<B>(
    tables: &KeyTables,
    request: &Request<B>,
    selector: &ContractSelector,
) -> Option<OpKeys> {
    if let Some(op) = rpc_op_id(request.uri().path())
        && let Some(keys) = tables.find(op, selector)
    {
        return Some(keys.clone());
    }
    let matched = request.extensions().get::<MatchedPath>()?.as_str();
    rest_candidates(request.method(), matched)
        .find_map(|key| tables.find(&key, selector))
        .cloned()
}

/// The `op_id` of `.../rpc/{op_id}`; never `batch` or a subscribe path.
fn rpc_op_id(path: &str) -> Option<&str> {
    let mut segments = path.rsplit('/');
    let op = segments.next()?;
    (segments.next()? == "rpc" && op != "batch").then_some(op)
}

fn rest_candidates<'a>(method: &'a Method, matched: &'a str) -> impl Iterator<Item = String> + 'a {
    matched
        .match_indices('/')
        .map(move |(at, _)| format!("{method} {}", &matched[at..]))
}
