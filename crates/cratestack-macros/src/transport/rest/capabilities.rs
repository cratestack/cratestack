//! The `RouteTransportCapabilities` literals each REST descriptor embeds:
//! which content types a route accepts and produces, and whether it can
//! answer with a `cbor-seq` sequence.
//!
//! Split out of `rest.rs` when `idempotent_by_default` pushed that file
//! past the 200-line ceiling (`CLAUDE.md`), retiring its
//! `.ci/file-length-allowlist.toml` entry rather than growing under it.
//! Moved verbatim — these three functions never referenced anything else
//! in `rest.rs`, which is why they were the clean cut.

use cratestack_core::{Procedure, TypeArity};
use quote::quote;

pub(crate) fn procedure_transport_capabilities_tokens(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &[
                    "application/cbor",
                    "application/json",
                    ::cratestack::CBOR_SEQUENCE_CONTENT_TYPE,
                ],
                default_response_type: "application/cbor",
                supports_sequence_response: true,
            }
        }
    } else {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &["application/cbor", "application/json"],
                default_response_type: "application/cbor",
                supports_sequence_response: false,
            }
        }
    }
}

pub(crate) fn model_read_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &[],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}

pub(crate) fn model_write_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &["application/cbor", "application/json"],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}
