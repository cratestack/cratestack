//! Compile-time proof that code outside `cratestack-core` cannot forge an
//! [`AbortOwnership::Claimed`](crate::AbortOwnership) abort — the only
//! state whose `Idempotency-Key` an idempotency layer releases
//! (GHSA-r67q-4qqq-g9gm, docs/design/procedure-isolation.md §6). The one
//! way to `Claimed` is the `#[doc(hidden)]`
//! `CratestackError::__generated_claim_transaction_abort`, which generated
//! dispatch calls.
//!
//! Stable rustdoc does not check the error code of a `compile_fail` block,
//! so each block below is this compiling twin with exactly one line
//! changed, and fails for that line alone; keep them in step. The twin
//! names `Claimed` (harmless: nothing public accepts it) and reads the
//! ownership through the getter:
//!
//! ```
//! use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
//!
//! let mut abort = TransactionAbort::exhausted(DbErrorInfo::default());
//! abort.info.sqlstate = Some("40001".to_owned());
//! let _ = AbortOwnership::Claimed;
//! assert_ne!(abort.ownership(), AbortOwnership::Claimed);
//! let error = CratestackError::TransactionAborted(abort);
//! assert!(error.is_idempotency_replayable());
//! ```
//!
//! Assigning the private field on an existing abort (E0616):
//!
//! ```compile_fail,E0616
//! use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
//!
//! let mut abort = TransactionAbort::exhausted(DbErrorInfo::default());
//! abort.info.sqlstate = Some("40001".to_owned());
//! abort.ownership = AbortOwnership::Claimed;
//! assert_ne!(abort.ownership(), AbortOwnership::Claimed);
//! let error = CratestackError::TransactionAborted(abort);
//! assert!(error.is_idempotency_replayable());
//! ```
//!
//! A struct literal (E0451, private field; the struct is also
//! `#[non_exhaustive]`):
//!
//! ```compile_fail,E0451
//! use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
//!
//! let mut abort = TransactionAbort { info: DbErrorInfo::default(), ownership: AbortOwnership::Claimed };
//! abort.info.sqlstate = Some("40001".to_owned());
//! let _ = AbortOwnership::Claimed;
//! assert_ne!(abort.ownership(), AbortOwnership::Claimed);
//! let error = CratestackError::TransactionAborted(abort);
//! assert!(error.is_idempotency_replayable());
//! ```
//!
//! Struct-update syntax over an existing abort (E0451):
//!
//! ```compile_fail,E0451
//! use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
//!
//! let mut abort = TransactionAbort::exhausted(DbErrorInfo::default());
//! abort.info.sqlstate = Some("40001".to_owned());
//! let mut abort = TransactionAbort { ownership: AbortOwnership::Claimed, ..abort };
//! assert_ne!(abort.ownership(), AbortOwnership::Claimed);
//! let error = CratestackError::TransactionAborted(abort);
//! assert!(error.is_idempotency_replayable());
//! ```
//!
//! A constructor that takes an ownership: there is none (E0599):
//!
//! ```compile_fail,E0599
//! use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
//!
//! let mut abort = TransactionAbort::new(DbErrorInfo::default(), AbortOwnership::Claimed);
//! abort.info.sqlstate = Some("40001".to_owned());
//! let _ = AbortOwnership::Claimed;
//! assert_ne!(abort.ownership(), AbortOwnership::Claimed);
//! let error = CratestackError::TransactionAborted(abort);
//! assert!(error.is_idempotency_replayable());
//! ```
