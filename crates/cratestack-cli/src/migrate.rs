//! `cratestack migrate` subcommands.
//!
//! Slice 5 ships `diff`. `verify` (replay against ephemeral DB) and
//! `drift` (introspect live DB) land in subsequent slices.

mod backend;
mod diff_cmd;
mod slug;

#[cfg(test)]
mod tests_diff;

pub(crate) use diff_cmd::handle_diff;
