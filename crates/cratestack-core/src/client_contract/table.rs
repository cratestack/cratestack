//! The digest tables generated code carries, and how a route finds its row.
//!
//! A signed message binds the digest of **the op it calls** (binding
//! version 2). The client holds one digest per op ([`OpContracts`]); a
//! server holds, per op, every digest it accepts ([`AcceptedContracts`]),
//! current first and then any older ones still wire-compatible with it,
//! newest first. Today the list has one member; the compatible-contract
//! lock (cratestack#1123, PR 3) only adds members, so nothing that reads
//! this table changes when it lands.
//!
//! Both tables are keyed by the op key of [`crate::op_list`] (the RPC
//! `op_id`, or `"<METHOD> <route template>"` on REST), plus one extra row
//! under [`BATCH_CONTRACT_KEY`] for an RPC schema: a signed `/rpc/batch`
//! has one AAD and many ops, so until it carries per-frame digests it
//! binds the whole [`client_contract_digest`], accepted only when equal.

use crate::schema::{Schema, TransportStyle};

use super::{client_contract_digest, op_contract_digests};

/// The row of a signed `/rpc/batch`: the route the AAD already binds.
pub const BATCH_CONTRACT_KEY: &str = "batch";

/// What a client stamps: one digest per op (and `batch`), sorted by key.
pub type OpContracts = &'static [(&'static str, [u8; 32])];

/// What a server accepts: per op (and `batch`), the digests a request may
/// bind, current first, then older compatible ones newest first.
pub type AcceptedContracts = &'static [(&'static str, &'static [[u8; 32]])];

/// The digest a call to `method route` binds, for every key of `schema`
/// (what the macros emit as `OP_CONTRACTS`): each op's digest, plus the
/// `batch` row for `transport rpc`. Sorted by key.
pub fn bound_contracts(schema: &Schema) -> Vec<(String, [u8; 32])> {
    let mut table = op_contract_digests(schema);
    if schema.transport == TransportStyle::Rpc {
        table.push((
            BATCH_CONTRACT_KEY.to_owned(),
            client_contract_digest(schema),
        ));
        table.sort_by(|a, b| a.0.cmp(&b.0));
    }
    table
}

/// The row a call belongs to: an RPC route is its `op_id` (or `batch`); a
/// REST route is bound as the template, and keyed `"<METHOD> <template>"`.
/// No allocation: an RPC id never starts with a method and a space. A
/// `HEAD` is the `GET` op's (axum serves it from that route; the binding
/// still names the method it was sent with).
///
/// ```
/// use cratestack_core::find_contract;
///
/// let table = [("model.Widget.list", 1), ("GET /widgets/{id}", 2)];
/// assert_eq!(find_contract(&table, "POST", "model.Widget.list"), Some(&1));
/// assert_eq!(find_contract(&table, "GET", "/widgets/{id}"), Some(&2));
/// assert_eq!(find_contract(&table, "HEAD", "/widgets/{id}"), Some(&2));
/// assert_eq!(find_contract(&table, "DELETE", "/widgets/{id}"), None);
/// ```
pub fn find_contract<'t, T>(
    table: &'t [(&'static str, T)],
    method: &str,
    route: &str,
) -> Option<&'t T> {
    let method = if method == "HEAD" { "GET" } else { method };
    table
        .iter()
        .find(|(key, _)| {
            *key == route
                || key
                    .strip_prefix(method)
                    .and_then(|rest| rest.strip_prefix(' '))
                    == Some(route)
        })
        .map(|(_, value)| value)
}
