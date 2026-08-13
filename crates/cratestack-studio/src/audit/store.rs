//! Optional append-only JSONL sink behind the audit ring buffer.
//!
//! Two decisions worth spelling out, because both are load-bearing for
//! what Studio promises:
//!
//! **The sink is Studio-local, never the target database.** Studio
//! reads (and, in `rw` mode, writes) rows the operator already owns;
//! it has never created a table. Persisting audit rows into the
//! target's own schema would turn a read-mostly admin UI into a tool
//! that migrates the user's database behind their back, and it would
//! be wrong twice over for `ro` targets and for API-only targets that
//! have no database at all. So the sink is a file next to
//! `studio.toml`, and the target database is left exactly as Studio
//! found it.
//!
//! **It is opt-in.** Studio's default footprint on the operator's
//! filesystem is zero; silently starting to write a log into someone's
//! repo is a behavioural change they did not ask for. Setting
//! `[workspace] audit_file` is that ask.
//!
//! The format is JSONL — one [`AuditEntry`] per line, append-only,
//! never rewritten or rotated by Studio. Append-only is what makes the
//! file cheap to write under a lock and safe to `tail -f`; not
//! rotating it is a deliberate non-feature, since an audit log that
//! silently discards its own history is worse than no audit log. The
//! *in-memory* ring is still capped, so the API surface stays bounded
//! however large the file grows.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::AuditEntry;

#[derive(Debug, thiserror::Error)]
pub enum AuditStoreError {
    #[error("failed to create audit log directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read audit log '{path}': {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open audit log '{path}' for append: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What [`AuditStore::open`] recovered from an existing file.
#[derive(Debug, Default)]
pub struct Replay {
    /// The newest entries, oldest-first, already truncated to the
    /// caller's requested cap.
    pub entries: Vec<AuditEntry>,
    /// Highest `id` seen anywhere in the file — including entries the
    /// cap dropped — so resumed ids never collide with historical ones.
    pub max_id: u64,
    /// Lines that failed to parse. Surfaced so the caller can warn
    /// without us deciding on its behalf that a half-written tail is
    /// fatal.
    pub skipped_lines: usize,
}

#[derive(Debug)]
pub struct AuditStore {
    path: PathBuf,
    /// Append failures are logged once at `error`, then demoted. A
    /// broken sink must not turn every subsequent write into log spam,
    /// and must never fail the operator's actual CREATE/UPDATE/DELETE
    /// — losing an audit line is bad, refusing the write the operator
    /// asked for is worse.
    warned: AtomicBool,
}

impl AuditStore {
    /// Create any missing parent directories, replay the existing file
    /// (keeping at most `cap` of the newest entries), and return the
    /// sink ready for appends.
    ///
    /// Failures here are hard errors: the operator explicitly asked for
    /// persistence, so booting into a state where Studio silently isn't
    /// persisting would be a lie. Mid-flight append failures are
    /// handled the other way round — see [`AuditStore::append`].
    pub fn open(path: &Path, cap: usize) -> Result<(Self, Replay), AuditStoreError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| AuditStoreError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let replay = match File::open(path) {
            Ok(file) => read_replay(path, file, cap)?,
            // A missing file is the normal first-run case, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Replay::default(),
            Err(source) => {
                return Err(AuditStoreError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        // Prove up-front that we can actually append, rather than
        // discovering it on the operator's first write.
        open_for_append(path)?;

        Ok((
            Self {
                path: path.to_path_buf(),
                warned: AtomicBool::new(false),
            },
            replay,
        ))
    }

    /// Append one entry as a single JSON line.
    ///
    /// The handle is opened per call rather than held open for the
    /// process lifetime so that an operator who rotates or truncates
    /// the file out from under Studio gets subsequent lines in the new
    /// file. Studio writes at human click-rate, so the extra `open(2)`
    /// costs nothing worth optimising.
    pub fn append(&self, entry: &AuditEntry) {
        if let Err(e) = self.try_append(entry) {
            if !self.warned.swap(true, Ordering::Relaxed) {
                tracing::error!(
                    path = %self.path.display(),
                    error = %e,
                    "audit log append failed; the write itself succeeded but was not persisted \
                     (further append failures will be logged at debug level)"
                );
            } else {
                tracing::debug!(path = %self.path.display(), error = %e, "audit log append failed");
            }
        }
    }

    fn try_append(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())
    }
}

fn open_for_append(path: &Path) -> Result<File, AuditStoreError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| AuditStoreError::Open {
            path: path.to_path_buf(),
            source,
        })
}

/// Stream the file a line at a time, retaining only the newest `cap`
/// entries. Streaming (rather than reading the whole file) keeps boot
/// memory bounded by `cap` no matter how long the operator has let the
/// log grow.
fn read_replay(path: &Path, file: File, cap: usize) -> Result<Replay, AuditStoreError> {
    let mut kept: std::collections::VecDeque<AuditEntry> = std::collections::VecDeque::new();
    let mut max_id = 0;
    let mut skipped_lines = 0;

    for line in BufReader::new(file).lines() {
        let line = line.map_err(|source| AuditStoreError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        // A torn last line (Studio killed mid-append) or a hand-edit
        // must not brick boot: skip it, count it, keep the rest.
        let Ok(entry) = serde_json::from_str::<AuditEntry>(&line) else {
            skipped_lines += 1;
            continue;
        };
        max_id = max_id.max(entry.id);
        if cap > 0 {
            if kept.len() == cap {
                kept.pop_front();
            }
            kept.push_back(entry);
        }
    }

    Ok(Replay {
        entries: kept.into(),
        max_id,
        skipped_lines,
    })
}
