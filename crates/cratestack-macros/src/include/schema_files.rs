//! `SCHEMA_FILES`, emitted by all three include macros for a schema that
//! spans more than one file (docs/design/multi-file-schemas.md §10).

use proc_macro2::TokenStream;
use quote::quote;

use super::parse::IncludedFile;

/// Empty for a single-file schema, so its generated output stays
/// byte-identical to what it was before multi-file schemas existed.
pub(super) fn schema_files_const(files: &[IncludedFile]) -> TokenStream {
    if files.len() <= 1 {
        return TokenStream::new();
    }
    let entries = files.iter().map(|file| {
        let relative = &file.manifest_relative;
        let absolute = file.absolute.display().to_string();
        quote! { (#relative, include_str!(#absolute)) }
    });
    quote! {
        /// Every file of this schema, `(path relative to CARGO_MANIFEST_DIR,
        /// contents)`, root first, in load order
        /// (docs/design/multi-file-schemas.md §7, §10). Each `include_str!`
        /// makes that file a rebuild dependency of this crate.
        pub const SCHEMA_FILES: &[(&str, &str)] = &[#(#entries),*];
    }
}

#[cfg(test)]
#[path = "schema_files_tests.rs"]
mod tests;
