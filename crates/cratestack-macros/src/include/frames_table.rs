//! `SUBSCRIPTION_FRAMES`, the generated table of the subscriptions a schema
//! marks `@frames(unsigned)` (cratestack#1378, part A).
//!
//! Server module only, beside `ACCEPTED_CONTRACTS`, keyed like it by op key.
//! The generated `envelope_layer` hands it to the layer, which only stores
//! it for now: the table is inert until the signed subscription open lands.
//! An unmarked subscription has no row, and a schema with none emits an
//! empty table, which a consumer must read as "no subscription may open
//! signed" (fail closed).

use proc_macro2::TokenStream;
use quote::quote;

use cratestack_core::FramesMode;

fn mode(mode: FramesMode) -> TokenStream {
    match mode {
        FramesMode::Unsigned => quote! { ::cratestack::FramesMode::Unsigned },
        // `FramesMode` is `#[non_exhaustive]`: a mode added upstream must
        // be given its tokens here before a schema can use it.
        other => unreachable!("no generated spelling for frames mode {other:?}"),
    }
}

/// `pub const SUBSCRIPTION_FRAMES`: op key to frames mode.
pub(super) fn subscription_frames(schema: &cratestack_core::Schema) -> TokenStream {
    rows(&cratestack_core::subscription_frames(schema))
}

fn rows(table: &[(String, FramesMode)]) -> TokenStream {
    let rows = table.iter().map(|(key, m)| {
        let m = mode(*m);
        quote! { (#key, #m) }
    });
    quote! {
        /// Per subscription the schema marks `@frames(unsigned)`, its op key
        /// (the RPC `op_id`, or `"<METHOD> <route template>"` on REST) and
        /// frames mode. Unmarked subscriptions have no row. The generated
        /// `axum::envelope_layer` passes it to the layer (cratestack#1378).
        pub const SUBSCRIPTION_FRAMES: &[(&str, ::cratestack::FramesMode)] = &[#(#rows),*];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_keyed_by_op_key_and_an_empty_table_is_emitted_empty() {
        let emitted = rows(&[("procedure.feed".to_owned(), FramesMode::Unsigned)]).to_string();
        assert!(
            emitted.contains(
                "pub const SUBSCRIPTION_FRAMES : & [(& str , :: cratestack :: FramesMode)]"
            ),
            "{emitted}"
        );
        assert!(
            emitted.contains("(\"procedure.feed\" , :: cratestack :: FramesMode :: Unsigned)"),
            "{emitted}"
        );
        let empty = rows(&[]).to_string();
        assert!(empty.ends_with("= & [] ;"), "{empty}");
    }
}
