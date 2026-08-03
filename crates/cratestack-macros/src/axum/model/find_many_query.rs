//! `FindMany<Model>` procedure-argument support: a `pub fn` wrapper per
//! model that turns a decoded `FindManyInput<Model>` into a real,
//! filtered/sorted query builder — reusing the model's own already-
//! generated `#list_builder_ident`/`ModelListQuery` machinery (the same
//! one `handle_list_<plural>_dispatch` already uses) rather than
//! reimplementing filter/sort parsing. Split out from `serializers.rs`
//! per the repo's 200-LoC file convention.

use quote::quote;

use crate::shared::{ident, to_snake_case};

use super::prep::ModelHandlerPrep;

pub(super) fn build_find_many_query_helper(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let find_many_query_ident = ident(&format!(
        "build_{}_query_from_find_many",
        to_snake_case(&p.model_name)
    ));
    let list_builder_ident = &p.list_builder_ident;
    let model_ident = &p.model_ident;
    let primary_key_type = &p.primary_key_type;

    quote! {
        /// Converts a decoded `FindMany<Model>` procedure argument into a
        /// ready-to-run query builder for this model — the `where`/
        /// `orderBy` grammar matches this model's own generated `list`
        /// route's `?where=`/`?sort=` query parameters exactly, validated
        /// against the same allowed fields. Call `.paginate(PageInput)`
        /// or `.run()` on the result. `pub` (unlike the list-builder it
        /// wraps): procedure implementations live in a separate app
        /// crate, not this generated module.
        pub fn #find_many_query_ident<'a>(
            db: &'a super::Cratestack,
            input: &::cratestack::FindManyInput<super::models::#model_ident>,
        ) -> Result<::cratestack::FindMany<'a, super::models::#model_ident, #primary_key_type>, CoolError> {
            let filters = match &input.r#where {
                Some(raw) => vec![::cratestack::parse_filter_expression(raw)?],
                None => Vec::new(),
            };
            let query = ModelListQuery {
                selection: ModelSelectionQuery::default(),
                limit: None,
                offset: None,
                sort: input.order_by.clone(),
                filters,
            };
            #list_builder_ident(db, &query, false)
        }
    }
}
