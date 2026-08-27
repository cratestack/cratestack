//! Shared harness for the integration tests that execute generated
//! TypeScript under a real Node.
//!
//! Exists because of cratestack#738. Five test files in this directory used
//! to invoke `npx --yes tsx@4.23.12 <script>` (seven call sites in total).
//! npm derives the `~/.npm/_npx/<hash>` directory from the package spec
//! alone, so every one of those invocations — across five *separate test
//! binaries* that `cargo test` runs concurrently — resolved to the identical
//! `~/.npm/_npx/95c8da6ffd4052b6`, then installed into and (on any failure)
//! rolled back out of it. That one shared, mutable directory produced three
//! distinct CI signatures from a single cause: `ERR_MODULE_NOT_FOUND` on
//! `tsx/dist/loader.mjs`, `npm warn cleanup ENOTEMPTY`, and
//! `npm error code ENOENT / syscall spawn sh`.
//!
//! The fix takes the issue's second Expected Behavior direction — remove
//! `npx` from the concurrent path entirely rather than give each racer its
//! own cache — because per-test caches only make two racers politer while
//! still paying a cold download each, whereas `tsx::command()` resolves the
//! runner **once per target directory** into an immutable, atomically
//! published tree that no test ever writes to. See `tsx.rs` for the
//! publication mechanism.
//!
//! `report.rs` closes the separate diagnostic gap the issue's second comment
//! documents: a failed smoke script used to panic with `smoke script failed:`
//! and two empty streams, giving a reader nothing to attribute the failure
//! to. Every subprocess assertion in these tests now reports the command,
//! its working directory, and its exit status alongside the streams.

mod report;
mod tsx;

pub use report::command_report;
pub use tsx::{node_toolchain_available, tsx_command};
