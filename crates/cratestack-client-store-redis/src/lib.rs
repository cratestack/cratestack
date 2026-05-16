//! Redis-backed `ClientStateStore` for `cratestack-client-rust`.

mod config;
mod store;

pub use config::RedisStateStoreConfig;
pub use store::RedisStateStore;
