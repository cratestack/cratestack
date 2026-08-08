//! Diffs `Projections::declared_extensions` into `Op::EnsureExtension`s.
//!
//! Only extensions with a real Postgres extension behind them produce
//! an op (`rate_limit` has no database counterpart — see
//! `docs/design/extensions.md` §6). The op is emitted once, the first
//! time a schema transitions from *not* declaring the extension to
//! declaring it — re-declaring across later diffs doesn't re-emit it.

use cratestack_core::ExtensionKind;

use crate::ir::{EnsureExtension, Op};
use crate::projection::Projections;

pub(super) fn diff_extensions(prev: &Projections, next: &Projections) -> Vec<Op> {
    next.declared_extensions
        .iter()
        .filter(|kind| !prev.declared_extensions.contains(kind))
        .filter_map(|kind| postgres_extension_name(*kind))
        .map(|name| {
            Op::EnsureExtension(EnsureExtension {
                name: name.to_owned(),
            })
        })
        .collect()
}

fn postgres_extension_name(kind: ExtensionKind) -> Option<&'static str> {
    match kind {
        ExtensionKind::Pgvector => Some("vector"),
        ExtensionKind::RateLimit => None,
        #[allow(unreachable_patterns)]
        _ => None,
    }
}
