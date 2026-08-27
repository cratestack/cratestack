//! Publishes a fully-populated staging directory at a stable path, tolerating
//! a destination that already exists in *any* state.
//!
//! ## Why the destination is routinely unusable in CI (cratestack#738 follow-up)
//!
//! The first cut of this assumed a failed `rename` implied a concurrent
//! publisher had won, and asserted the destination was usable. That is wrong,
//! and it turned `main` red on `248fc7ee` (run 33036199009): `ENOTEMPTY` plus a
//! destination that never becomes usable is not a race, it is the normal state
//! of a CI runner that restored a cached `target/`.
//!
//! `Swatinem/rust-cache`'s `cleanTargetDir` (see `src/cleanup.ts` at the pinned
//! `6323deb`) classifies any directory under `target/` lacking a
//! `build`/`.fingerprint`/`deps` child as a *nested target directory* and
//! recurses into it, deleting every non-directory entry it meets. Our tree
//! under `CARGO_TARGET_TMPDIR` matches that description at every level, so the
//! save step walks `target/tmp/tsx-<version>/node_modules/tsx/dist/` and
//! unlinks `cli.mjs` along with every other regular file, keeping the
//! directory skeleton. The gutted skeleton is then saved into the cache and
//! restored by the *next* run — which is why this passed on the merge commit
//! and failed on the commit after it.
//!
//! So a pre-existing, non-empty, unusable destination is not an edge case here;
//! on CI it is the *expected* input. It also arises from an interrupted run.
//!
//! ## The invariant
//!
//! A reader must only ever observe the published path **absent or complete** —
//! never half-written, and never being emptied in place. Every mutation below
//! is therefore a `rename`: the destination is swapped out under a private
//! name and only destroyed once it has been re-checked *in private*, which is
//! what keeps a concurrent publisher's good tree from being deleted out from
//! under a reader that is about to exec from it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Two passes suffice for the real sequences (condemn-then-retry, or
/// adopt-a-winner); a third is slack. Bounded rather than a spin loop, so a
/// genuinely stuck filesystem fails with a message instead of hanging — this
/// harness has a documented history of hangs being worse than failures
/// (cratestack#726).
const ATTEMPTS: usize = 3;

/// Moves `staging` to `published`, or adopts an equivalent tree already there.
///
/// `is_complete` validates a candidate directory by the artifact the caller
/// actually executes, not by mere existence — a skeleton of empty directories
/// must not pass.
pub fn publish_tree(staging: &Path, published: &Path, is_complete: &dyn Fn(&Path) -> bool) {
    for _ in 0..ATTEMPTS {
        // Destination absent, or an empty directory: `rename` handles both.
        if std::fs::rename(staging, published).is_ok() {
            return;
        }
        // Destination is a non-empty directory. If it is usable, a concurrent
        // publisher won and its tree is as good as ours.
        if is_complete(published) {
            discard(staging);
            return;
        }
        // Unusable. Swap it out under a private name rather than deleting in
        // place, so no reader can ever see a partially-emptied destination.
        let condemned = sibling(published, "condemned");
        if std::fs::rename(published, &condemned).is_err() {
            // Another process moved or replaced it first; re-evaluate.
            continue;
        }
        if is_complete(&condemned) {
            // It became usable between the check above and the swap. It is a
            // real tree, so put it back and let the next pass adopt it.
            let _ = std::fs::rename(&condemned, published);
            continue;
        }
        discard(&condemned);
    }

    discard(staging);
    assert!(
        is_complete(published),
        "could not publish to {} after {ATTEMPTS} attempts, and it is still not usable",
        published.display()
    );
}

/// Best-effort removal of a path we own, whatever kind of thing it turned out
/// to be.
///
/// Tries the directory form first because that is what every path here is
/// supposed to be, then falls back to `remove_file` — a destination left as a
/// regular file by some other tool would otherwise survive `remove_dir_all`
/// and leak. Failures are ignored on purpose: this only ever runs against a
/// path already established as unusable, so failing to clean it up must not
/// fail a test.
fn discard(path: &Path) {
    if std::fs::remove_dir_all(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
}

/// A private sibling of `path`, unique per process and call.
///
/// A sibling (not a tempdir) so it lands on the same filesystem and the
/// `rename` is atomic rather than a cross-device copy.
pub fn sibling(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().map_or_else(
        || "tree".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    path.with_file_name(format!(".{name}.{tag}-{}-{nanos}", std::process::id()))
}
