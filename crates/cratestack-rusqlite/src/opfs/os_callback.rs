//! Bridges `sqlite-wasm-rs` 0.5's OS services to the `rsqlite-vfs` 0.2
//! `OsCallback` that `sqlite-wasm-vfs` 0.3's SAH-pool VFS is generic over.
//!
//! rusqlite 0.40 links `sqlite-wasm-rs` 0.5 on wasm32, whose `WasmOsCallback`
//! implements `rsqlite-vfs` 0.1's trait (static functions). `sqlite-wasm-vfs`
//! 0.3 wants 0.2's (`&self` methods, plus `Default`). Nothing else differs:
//! 0.3 does not depend on `sqlite-wasm-rs` (only as a dev-dependency), and the
//! SQLite functions it calls (`sqlite3_vfs_{find,register,unregister}`,
//! `sqlite3_uri_{parameter,boolean,int64}`) are all exported by 0.5's build
//! (`SQLITE_USE_URI`, no symbol prefix). So the VFS registers into the one
//! SQLite that rusqlite opens. This adapter delegates to 0.5's own
//! implementations, so sleep, randomness and the clock behave exactly as
//! before. Drop it once rusqlite moves to `sqlite-wasm-rs` 0.6.

use core::time::Duration;

use rsqlite_vfs::{OsCallback, VfsResult};
use rsqlite_vfs_01::OsCallback as OsCallback01;
use sqlite_wasm_rs::WasmOsCallback;

/// `sqlite-wasm-rs` 0.5's `WasmOsCallback`, seen through `rsqlite-vfs` 0.2.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct BridgedOsCallback;

impl OsCallback for BridgedOsCallback {
    fn sleep(&self, dur: Duration) {
        <WasmOsCallback as OsCallback01>::sleep(dur);
    }

    fn random(&self, buf: &mut [u8]) -> usize {
        // 0.5 always fills the whole buffer (it falls back to a
        // non-cryptographic source when `crypto.getRandomValues` is missing),
        // which is the behaviour SQLite had with `sqlite-wasm-vfs` 0.2.
        <WasmOsCallback as OsCallback01>::random(buf);
        buf.len()
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        Ok(<WasmOsCallback as OsCallback01>::epoch_timestamp_in_ms())
    }
}
