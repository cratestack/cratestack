//! `db.transaction(...)` combinator (cratestack#513): compose several
//! write-builder calls in one Postgres transaction using only CrateStack's
//! own API — no `sqlx` dependency in the caller's `Cargo.toml`, and no
//! `sqlx::Transaction` named in the caller's own source.
//!
//! **Design (see the PR body for the sub-questions this answers in full):**
//!
//! - [`Tx`] is an opaque, crate-owned newtype around
//!   `sqlx::Transaction<'static, sqlx::Postgres>`. It implements
//!   [`Deref`]/[`DerefMut`] to that type, which is what lets every existing
//!   `run_in_tx(&mut sqlx::Transaction<'tx, Postgres>, ctx)` on the write
//!   builders (`create.rs`, `update.rs`, ...) keep their exact current
//!   signature: passing `&mut Tx` at a `&mut sqlx::Transaction<'_, _>` call
//!   site coerces automatically via `DerefMut`, so nothing downstream of
//!   `run_in_tx` had to change. The caller's closure never has to name
//!   `sqlx::Transaction` (or even `Tx` — the type is inferred), it's not a
//!   breaking change to `run_in_tx`, and the transaction still round-trips
//!   through the real `sqlx` machinery underneath.
//! - The closure is bound by `AsyncFnOnce(&mut Tx) -> Result<T, CoolError>`
//!   (the native async-closure traits stabilized in Rust 1.85; this
//!   workspace pins 1.95) rather than the `FnMut(...) -> Fut` shape
//!   `run_in_isolated_tx` uses. That older shape requires the body to hand
//!   the transaction *back* out of the future on every call (see
//!   `isolation.rs`) because a plain closure returning `async move { .. }`
//!   can't express "the returned future borrows the argument for its own
//!   lifetime" — the classic Rust lending-closure problem. `AsyncFnOnce`
//!   solves exactly that: callers can write
//!   `db.transaction(async |tx| { ...; Ok(value) }).await` and reuse `tx`
//!   across as many sequential `.await`s as they like without threading it
//!   back through the return type. Verified against a standalone
//!   reproduction before adopting it here — see the PR body.
//! - No retry loop: unlike [`crate::run_in_isolated_tx`], `transaction`
//!   doesn't re-run `body` on a serialization failure, since `body` isn't
//!   guaranteed idempotent (it's arbitrary caller code, not caller code
//!   already scoped to "safe to retry" the way `@isolation` procedures
//!   are). Retrying is exactly what `run_in_isolated_tx` is for; the two
//!   are orthogonal and composable (see the PR body's isolation
//!   discussion), not alternatives to pick between.

use std::ops::{Deref, DerefMut};

use cratestack_core::CoolError;

use crate::descriptor::SqlxRuntime;
use crate::error::cool_error_from_sqlx;
use crate::sqlx;

/// Opaque handle onto a live Postgres transaction. Obtained only via
/// [`SqlxRuntime::transaction`]; never constructed directly by consumers.
///
/// Derefs to `sqlx::Transaction<'static, sqlx::Postgres>` purely so the
/// existing write-builder `run_in_tx` methods keep working unchanged (see
/// the module doc comment) — this is an implementation detail, not an
/// invitation to import `sqlx` yourself. Nothing about the public
/// `db.transaction(...)` call site requires it.
pub struct Tx(sqlx::Transaction<'static, sqlx::Postgres>);

impl Deref for Tx {
    type Target = sqlx::Transaction<'static, sqlx::Postgres>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Tx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl SqlxRuntime {
    /// Run `body` inside one Postgres transaction: commit if it returns
    /// `Ok`, roll back if it returns `Err`. `body` receives an opaque [`Tx`]
    /// it can pass straight through to any write builder's `run_in_tx` —
    /// see the module doc comment for why no `sqlx` type ever needs to be
    /// named to do that.
    ///
    /// On the `Err` path this issues an explicit `tx.rollback().await`
    /// rather than relying on `sqlx::Transaction`'s `Drop` impl. That
    /// matters: `sqlx-core`'s `Drop for Transaction` only *queues* a
    /// rollback for the next time the underlying connection is used (see
    /// `sqlx-core::transaction::Transaction`'s `Drop` impl) — it does not
    /// synchronously roll back. A caller asserting "neither write is
    /// visible" immediately after an `Err` return needs that to have
    /// already happened, not to be pending on some future unrelated query.
    ///
    /// Does not retry — see the module doc comment for why that's left to
    /// [`crate::run_in_isolated_tx`] instead, and how the two compose.
    pub async fn transaction<F, T>(&self, body: F) -> Result<T, CoolError>
    where
        F: AsyncFnOnce(&mut Tx) -> Result<T, CoolError>,
    {
        let inner = self.pool().begin().await.map_err(cool_error_from_sqlx)?;
        let mut tx = Tx(inner);

        match body(&mut tx).await {
            Ok(value) => {
                tx.0.commit().await.map_err(cool_error_from_sqlx)?;
                Ok(value)
            }
            Err(error) => {
                // Best-effort: if the rollback itself fails (e.g. the
                // connection already dropped), the original `error` is
                // still the one that matters to the caller — a failed
                // rollback attempt shouldn't mask it.
                let _ = tx.0.rollback().await;
                Err(error)
            }
        }
    }
}
