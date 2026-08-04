//! `cratestack migrate` subcommands.
//!
//! Slice 5 shipped `diff`. Issue #205 adds `baseline` (adopt an
//! already-existing database). `verify` (replay against an ephemeral
//! DB) is still unimplemented.

mod backend;
mod baseline_cmd;
mod diff_cmd;
mod drift_report;
mod slug;

#[cfg(test)]
mod tests_baseline;
#[cfg(test)]
mod tests_diff;

pub(crate) use baseline_cmd::handle_baseline;
pub(crate) use diff_cmd::handle_diff;
