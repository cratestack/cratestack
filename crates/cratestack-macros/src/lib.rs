mod axum;
mod client;
mod event;
mod include;
mod model;
mod policy;
mod procedure;
mod relation;
mod shared;
mod transport;
mod types;

use proc_macro::TokenStream;

#[proc_macro]
pub fn include_schema(input: TokenStream) -> TokenStream {
    include::include_schema(input)
}
