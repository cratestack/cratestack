//! The client's compact-key tables (`KEY_TABLES`,
//! docs/design/compact-wire-keys.md §5.8): every op at its current
//! contract, built at macro-expansion time and embedded as one byte string.
//! The generated `Client::with_compact_keys` hands it to the runtime.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

pub(super) fn emit(schema: &cratestack_core::Schema) -> TokenStream {
    let bytes =
        Literal::byte_string(&cratestack_core::compact::KeyTables::from_schema(schema).to_bytes());
    quote! {
        /// The compact-wire-keys artifact (`KeyTables::to_bytes`) of this
        /// schema's current contracts; see `client::Client::with_compact_keys`.
        pub const KEY_TABLES: &[u8] = #bytes;
    }
}
