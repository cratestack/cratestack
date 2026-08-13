//! cratestack#490 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.

use cratestack::client_rust::{CborCodec, ClientConfig, CratestackClient};

cratestack::include_client_schema!("schema.cstack");

pub use cratestack_schema as schema;

/// `CratestackClient::new` — no `axum::Router`, no `cratestack-axum`
/// dependency in this crate's graph to have built one against even if the
/// code wanted to. That's the whole point of this crate.
pub fn build_client(base_url: url::Url) -> schema::client::Client {
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    schema::client::Client::new(runtime)
}
