//! `<schema>.pb.lock` — the protobuf field-number lockfile.
//!
//! Everything downstream of a `.proto` emission (ticket #169 and later)
//! needs a stable field number per field, enum variant, and message. This
//! crate owns the lock's data model and the assignment/reservation
//! algorithm described in `docs/design/protobuf.md` §3.3 — no `.proto` text
//! emission lives here, and no file I/O: callers own reading and writing
//! the lock file, this crate only builds and (de)serializes its contents.
//!
//! See [`build_lock`] for the algorithm and [`PbLock`] for the shape.

mod casing;
mod emit;
mod lock;

pub use casing::op_id_to_method_name;
pub use emit::{ProtoEmitError, emit_proto, synthesize_messages};
pub use lock::{EnumLock, MessageLock, PbLock, PbLockError, build_lock, lock_would_change};
