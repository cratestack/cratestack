//! The server's compact-key tables (`KEY_TABLES`, docs/design/compact-wire-keys.md
//! §5.8): every op at its current contract, plus every older contract the
//! `contracts = "..."` lock keeps accepting, so a client on an older release
//! is served with its own key numbering. Built here at macro-expansion time
//! and embedded as one byte string.

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::LitStr;

use cratestack_core::compact::KeyTables;
use cratestack_core::{ContractLock, Schema, digest_hex};

/// `schema`'s tables, with the locked contracts of `accepted` added when a
/// `lock` is given. A locked op the current schema no longer has (`batch`
/// included) has no table and is skipped.
pub(super) fn build(
    schema: &Schema,
    lock: Option<(&ContractLock, &LitStr)>,
    accepted: &[(String, Vec<[u8; 32]>)],
) -> Result<KeyTables, syn::Error> {
    let mut tables = KeyTables::from_schema(schema);
    let Some((lock, span)) = lock else {
        return Ok(tables);
    };
    for (op, digests) in accepted {
        if !tables.ops.iter().any(|keys| &keys.op == op) {
            continue;
        }
        for digest in digests {
            if tables.find_digest(op, digest).is_some() {
                continue;
            }
            let Some(contract) = lock.contracts.get(&digest_hex(digest)) else {
                continue;
            };
            tables.add_contract(*digest, contract).map_err(|error| {
                syn::Error::new(
                    span.span(),
                    format!("the locked contract of `{op}` has no compact key table: {error}"),
                )
            })?;
        }
    }
    Ok(tables)
}

/// `pub const KEY_TABLES: &[u8]`.
pub(super) fn emit(tables: &KeyTables) -> TokenStream {
    let bytes = Literal::byte_string(&tables.to_bytes());
    quote! {
        /// The compact-wire-keys artifact (`KeyTables::to_bytes`): every op at
        /// its current contract and at each older contract this server still
        /// accepts. The generated `compact_layer()` reads it.
        pub const KEY_TABLES: &[u8] = #bytes;
    }
}
