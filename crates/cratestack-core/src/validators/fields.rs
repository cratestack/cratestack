//! Validators declared on the fields of a `type`, enforced on procedure
//! arguments (ADR 0019 D5, PR A).
//!
//! A `type` field accepts `@length`, `@range`, `@regex`, `@email`, `@uri`
//! and `@iso4217` exactly as a model field does. A model runs them on its
//! create and update inputs; a `type` has no input struct, so the macros
//! implement this trait on every `type` that carries a validator, directly
//! or through a nested `type`, and on the `Args` of every procedure that
//! takes one. `authorize_with_db` calls it before the `@allow` check, so
//! every transport (REST, RPC unary, RPC batch, MCP) and every non-HTTP
//! caller of `invoke_with_db` is validated by the one call.
//!
//! Only arguments are validated. A procedure's return value is produced by
//! the server, so a validator on a type that is only ever returned is inert.

use crate::error::CratestackError;

pub trait ValidateFields {
    /// Run the validators of `self` and of everything it contains. `path` is
    /// where `self` sits in the request body, with a trailing `.` unless it
    /// is empty: `""` for the arguments, `"args."` for the value of the
    /// argument `args`, `"args.items[2]."` for its third item. The error
    /// names the field by that path, so a nested failure points at the
    /// offending leaf.
    fn validate_at(&self, path: &str) -> Result<(), CratestackError>;

    /// [`Self::validate_at`] from the root of the request body.
    fn validate(&self) -> Result<(), CratestackError> {
        self.validate_at("")
    }
}
