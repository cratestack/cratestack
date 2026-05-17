//! OPFS bootstrap for `wasm32-unknown-unknown`.
//!
//! `rusqlite 0.39` swaps its FFI to `sqlite-wasm-rs` on wasm32 transparently,
//! but it defaults to the in-memory VFS — persistent storage in the browser
//! requires installing the OPFS SAH-pool VFS first. This module wraps
//! [`sqlite_wasm_vfs::sahpool::install`] so callers don't need to depend on
//! the raw VFS crate directly.
//!
//! ## Worker requirement
//!
//! OPFS `SyncAccessHandle` is available **only inside a Dedicated Worker**
//! per the spec. Calling [`install_opfs_vfs`] from the main thread returns
//! [`OpfsInstallError::NotSupported`]. Browser apps wanting offline storage
//! must spawn a worker, run the runtime there, and `postMessage` between
//! main thread and worker.
//!
//! ## Usage
//!
//! ```ignore
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use cratestack_rusqlite::{RusqliteRuntime, opfs};
//!
//! opfs::install_opfs_vfs(&opfs::OpfsOptions::default()).await?;
//! let runtime = RusqliteRuntime::open("opfs-sahpool.db")?;
//! # Ok(()) }
//! ```
//!
//! The filename `"opfs-sahpool.db"` (or any name) maps to a file inside the
//! OPFS sub-directory configured by [`OpfsOptions::directory`]. The VFS is
//! registered as the default on first install, so subsequent `Connection::
//! open()` calls automatically route through it.

use sqlite_wasm_rs as ffi;
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfg, install as sahpool_install};

/// Options for the OPFS SAH-pool VFS.
#[derive(Debug, Clone)]
pub struct OpfsOptions {
    /// VFS name as registered with sqlite. Defaults to `"opfs-sahpool"`.
    pub vfs_name: String,
    /// OPFS sub-directory the pool uses for metadata. Defaults to `".opfs-sahpool"`.
    pub directory: String,
    /// Pre-allocated SyncAccessHandle count. Each open DB file (main + journal)
    /// consumes one. Defaults to 6.
    pub initial_capacity: u32,
    /// Wipe the pool on init. Useful for tests and "logged out, clear data"
    /// flows. Defaults to false.
    pub clear_on_init: bool,
    /// Register this VFS as the sqlite default so plain
    /// `Connection::open(filename)` routes through it. Defaults to true.
    pub set_as_default: bool,
}

impl Default for OpfsOptions {
    fn default() -> Self {
        Self {
            vfs_name: "opfs-sahpool".into(),
            directory: ".opfs-sahpool".into(),
            initial_capacity: 6,
            clear_on_init: false,
            set_as_default: true,
        }
    }
}

/// Errors surfaced by [`install_opfs_vfs`].
#[derive(Debug)]
pub enum OpfsInstallError {
    /// The runtime context isn't a Dedicated Worker (OPFS `SyncAccessHandle`
    /// is worker-only).
    NotSupported,
    /// The VFS install failed for any other reason — see the inner string.
    Vfs(String),
}

impl std::fmt::Display for OpfsInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSupported => {
                write!(f, "OPFS install requires a Dedicated Worker context")
            }
            Self::Vfs(message) => write!(f, "OPFS VFS install failed: {message}"),
        }
    }
}

impl std::error::Error for OpfsInstallError {}

/// Install the OPFS SAH-pool VFS so that `RusqliteRuntime::open(filename)`
/// persists across page reloads. Must run inside a Dedicated Worker.
pub async fn install_opfs_vfs(options: &OpfsOptions) -> Result<(), OpfsInstallError> {
    let cfg = OpfsSAHPoolCfg {
        vfs_name: options.vfs_name.clone(),
        directory: options.directory.clone(),
        clear_on_init: options.clear_on_init,
        initial_capacity: options.initial_capacity,
    };
    sahpool_install::<ffi::WasmOsCallback>(&cfg, options.set_as_default)
        .await
        .map(|_| ())
        .map_err(|error| {
            let text = format!("{error:?}");
            if text.contains("NotSupported") {
                OpfsInstallError::NotSupported
            } else {
                OpfsInstallError::Vfs(text)
            }
        })
}
