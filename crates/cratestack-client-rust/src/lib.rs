mod auth;
mod client;
mod codec;
mod config;
mod error;
mod projection;
mod rpc;
mod runtime;
mod state;
mod streaming;
mod streaming_callback;

#[cfg(test)]
mod tests;

pub use cratestack_codec_cbor::CborCodec;
pub use cratestack_codec_json::JsonCodec;
pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_UNARY_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame, rpc_code,
};

pub use auth::{AuthorizationRequest, RequestAuthorizer};
pub use client::CratestackClient;
pub use codec::HttpClientCodec;
pub use config::ClientConfig;
pub use error::{ClientError, HeaderPair, QueryPair};
pub use projection::Projection;
pub use rpc::batch::{BatchBuilder, BatchResults};
pub use rpc::batch_call::{BatchHandle, BatchableCall};
pub use rpc::client::RpcClient;
pub use rpc::error::{RpcClientError, RpcRemoteError, RpcStream};
pub use runtime::handle::RuntimeHandle;
pub use runtime::wire::{
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
    RuntimeErrorWire, RuntimeHeader, RuntimeRequestWire, RuntimeResponseWire,
    RuntimeStateStoreConfig, RuntimeTransportConfig,
};
pub use state::{
    ClientStateStore, InMemoryStateStore, JsonFileStateStore, PersistedClientState,
    RequestJournalEntry,
};
pub use streaming::CborSeqChunkDecoder;
pub use streaming_callback::RuntimeChunkWire;
