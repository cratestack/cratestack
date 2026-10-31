//! The generated Rust clients, one per binding, both signing through the
//! same [`Device`]: whatever token it holds is on the next request.

use std::sync::Arc;

use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use url::Url;

use crate::device::Device;

/// The REST client of `schema.cstack`.
pub mod rest {
    cratestack::include_client_schema!("schema.cstack");
}

/// The RPC client of `schema-rpc.cstack`.
pub mod rpc {
    cratestack::include_client_schema!("schema-rpc.cstack");
}

fn runtime(base_url: &Url, device: &Arc<Device>) -> CratestackClient<CborCodec> {
    CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
        .with_request_authorizer(device.clone())
}

/// A REST client at `base_url` that signs with `device`.
pub fn rest_client(
    base_url: &Url,
    device: &Arc<Device>,
) -> rest::cratestack_schema::client::Client {
    rest::cratestack_schema::client::Client::new(runtime(base_url, device))
}

/// An RPC client at `base_url` that signs with `device`.
pub fn rpc_client(base_url: &Url, device: &Arc<Device>) -> rpc::cratestack_schema::client::Client {
    rpc::cratestack_schema::client::Client::new(runtime(base_url, device))
}
