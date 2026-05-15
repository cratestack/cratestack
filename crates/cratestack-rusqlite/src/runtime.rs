//! `RusqliteRuntime` — the on-device storage handle.
//!
//! Owns a single `rusqlite::Connection` behind a `Mutex`. Mobile apps are
//! single-user; pooling adds binary size and a concurrency story we don't
//! need. If a future use case wants a pool, swap the `Mutex<Connection>`
//! for a connection-pool wrapper without touching the delegate code.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

/// Errors surfaced by the on-device runtime. Stays close to `rusqlite::Error`
/// for now — wrapping in a cratestack-specific variant only when we cross
/// the FFI boundary (Phase 5).
#[derive(Debug)]
pub enum RusqliteError {
    /// Underlying SQLite error.
    Sqlite(rusqlite::Error),
    /// Operation expected exactly one row but got a different count.
    NotFound,
    /// Locked or poisoned mutex around the connection.
    Locked,
    /// Batch request exceeded the documented per-call item cap.
    BatchTooLarge {
        actual: usize,
        maximum: usize,
    },
    /// Batch request contained the same primary key twice. The first/duplicate
    /// indices are surfaced so callers can immediately pinpoint the offender
    /// in their input list.
    DuplicateBatchKey {
        first: usize,
        duplicate: usize,
    },
    /// Caller-side input rejected before any SQL ran (e.g. `update_many`
    /// without a filter, an empty patch set). Distinct from a SQLite-level
    /// error so callers can surface a fast-fail validation message rather
    /// than a generic database error.
    Validation(String),
}

impl std::fmt::Display for RusqliteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "sqlite error: {error}"),
            Self::NotFound => write!(f, "not found"),
            Self::Locked => write!(f, "connection mutex poisoned"),
            Self::BatchTooLarge { actual, maximum } => write!(
                f,
                "batch size {actual} exceeds maximum of {maximum}",
            ),
            Self::DuplicateBatchKey { first, duplicate } => write!(
                f,
                "duplicate primary key in batch at positions {first} and {duplicate}",
            ),
            Self::Validation(message) => write!(f, "validation error: {message}"),
        }
    }
}

impl std::error::Error for RusqliteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for RusqliteError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

/// The on-device storage handle. Cheap to clone via `Arc` at the call site;
/// the runtime itself is not `Clone` because the underlying connection
/// shouldn't be silently duplicated.
pub struct RusqliteRuntime {
    conn: Mutex<Connection>,
}

impl RusqliteRuntime {
    /// Open an in-memory database. Intended for tests; mobile apps will
    /// almost always use [`Self::open`].
    pub fn open_in_memory() -> Result<Self, RusqliteError> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open or create a database at the given path. Applies the default
    /// pragmas (foreign keys on, journal mode WAL) appropriate for a
    /// mobile app's storage characteristics.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RusqliteError> {
        let conn = Connection::open(path)?;
        configure(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run a closure with exclusive access to the underlying connection.
    /// Use for migrations, multi-statement transactions, and anywhere the
    /// ORM delegate isn't expressive enough.
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, RusqliteError>
    where
        F: FnOnce(&mut Connection) -> Result<T, RusqliteError>,
    {
        let mut guard = self.conn.lock().map_err(|_| RusqliteError::Locked)?;
        f(&mut guard)
    }
}

fn configure(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // WAL is the right default for an app holding the file open across the
    // session — better read concurrency, fewer fsync stalls.
    // pragma_update returns an error if the journal mode pragma isn't
    // recognised (in-memory connections), so swallow that silently.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    Ok(())
}
