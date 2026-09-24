//! `__procedure_json_schemas!` — a `#[doc(hidden)]` test hook, not public
//! API, and no facade re-exports it.
//!
//! Phase 2 of the MCP operator (cratestack#1037) must verify generated
//! JSON Schemas against the real generated types, and those only exist in
//! a crate that expands `include_server_schema!` with a facade in scope.
//! This crate is a proc-macro, so its plain functions are unreachable
//! from there. A second macro that loads the same `.cstack` file the same
//! way (`parse_schema_literal`, `resolve_decimal_backend`) and returns the
//! schemas as `&'static str` is the narrowest bridge: it exercises the
//! generator at macro-expansion time, which is where phase 3 will call it.
//!
//! Phase 3's generated `mcp` module carries the schemas itself, at which
//! point this hook can go. It adds nothing to `include_*_schema!` output.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

use super::decimal_arg::resolve_decimal_backend;
use super::parse::{SchemaPathArgs, parse_schema_literal};
use crate::json_schema::{procedure_input_schema, procedure_output_schema};

/// Expands to a `&'static [(name, input, output)]`, one entry per
/// procedure in declaration order. `input` is `Ok(schema_json)` or
/// `Err(message)`; `output` is `Ok(None)` when the return type is not an
/// object. A generator error is data here, not a compile error, so tests
/// can assert on it.
pub(super) fn procedure_json_schemas(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as SchemaPathArgs);
    let (_, resolved, schema, _) = match parse_schema_literal(&args.schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let decimal = match resolve_decimal_backend(&args.schema_path, &schema, args.decimal) {
        Ok(decimal) => decimal,
        Err(error) => return error,
    };

    let entries = schema.procedures.iter().map(|procedure| {
        let name = &procedure.name;
        let input = match procedure_input_schema(&schema, procedure, decimal) {
            Ok(value) => {
                let json = value.to_string();
                quote! { ::core::result::Result::Ok(#json) }
            }
            Err(error) => {
                let message = error.to_string();
                quote! { ::core::result::Result::Err(#message) }
            }
        };
        let output = match procedure_output_schema(&schema, procedure, decimal) {
            Ok(Some(value)) => {
                let json = value.to_string();
                quote! { ::core::result::Result::Ok(::core::option::Option::Some(#json)) }
            }
            Ok(None) => quote! { ::core::result::Result::Ok(::core::option::Option::None) },
            Err(error) => {
                let message = error.to_string();
                quote! { ::core::result::Result::Err(#message) }
            }
        };
        quote! { (#name, #input, #output) }
    });
    let resolved = resolved.display().to_string();

    quote! {{
        // Recompile when the schema file changes, as the entry macros do.
        const _: &str = include_str!(#resolved);
        const SCHEMAS: &[(
            &str,
            ::core::result::Result<&str, &str>,
            ::core::result::Result<::core::option::Option<&str>, &str>,
        )] = &[#(#entries),*];
        SCHEMAS
    }}
    .into()
}
