//! Back-compat re-exports from cratestack-core. The trait and types moved to
//! cratestack-core::store::client_state to break the layering back-edge where
//! store adapters (L2) depended on the HTTP client (L4).

pub use cratestack_core::{
    ClientStateStore, InMemoryStateStore, JsonFileStateStore, PersistedClientState,
    RequestJournalEntry,
};
