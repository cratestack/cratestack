//! Everything a single verb group's builder needs — computed once by
//! `generate_generated_model_client` and shared read-only across
//! [`super::groups_read`]/[`super::groups_write`]'s builders, mirroring
//! `transport::rpc::model_dispatch::ModelRpcContext`'s split (also
//! cratestack#743, `docs/design/route-suppression.md`).

pub(super) struct ModelRestClientContext {
    pub(super) route_path: String,
    pub(super) primary_key_type: proc_macro2::TokenStream,
    pub(super) model_output_type: proc_macro2::TokenStream,
    pub(super) list_output_type: proc_macro2::TokenStream,
    pub(super) list_view_output_type: proc_macro2::TokenStream,
    pub(super) list_view_call: proc_macro2::TokenStream,
    pub(super) create_input_ident: syn::Ident,
    pub(super) update_input_ident: syn::Ident,
    pub(super) computed_params_ident: Option<syn::Ident>,
}
