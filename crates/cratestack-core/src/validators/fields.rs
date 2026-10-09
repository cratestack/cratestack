//! Validators declared on the fields of a `type` or a `model`, enforced on
//! client input that is not a model's create or update input (ADR 0019 D5,
//! PR A).
//!
//! A `type` or `model` field accepts `@length`, `@range`, `@regex`,
//! `@email`, `@uri` and `@iso4217`. A model runs them on its create and
//! update inputs. A `type` has no input struct, and a model reached as a
//! procedure argument is not an input, so the macros implement this trait on
//! every `type` and `model` that carries a validator, directly or through a
//! nested `type`, and on the `Args` of every procedure that takes one. The
//! generated `authorize`, `authorize_with_db`, `invoke` and `invoke_with_db`
//! each call it before the `@allow` check, so every transport (REST, RPC
//! unary, RPC batch, MCP) and every non-HTTP caller is validated, whichever
//! helper it picks. `?computedParams=` is client input too: the params
//! `type` of a `@computed` field is validated before the model is read.
//!
//! Only client input is validated. A procedure's return value is produced
//! by the server, so a validator on a `type` that no client input reaches
//! would be inert, and `cratestack check` refuses it.

use super::FieldPath;
use crate::error::CratestackError;

pub trait ValidateFields {
    /// Run the validators of `self` and of everything it contains. `path` is
    /// where `self` sits in the request body: [`FieldPath::Root`] for the
    /// arguments, `args` for the value of the argument `args`,
    /// `args.items[2]` for its third item. The error names the field by that
    /// path, so a nested failure points at the offending leaf; the path is
    /// written only when a validator fails.
    fn validate_at(&self, path: &FieldPath<'_>) -> Result<(), CratestackError>;

    /// [`Self::validate_at`] from the root of the request body.
    fn validate(&self) -> Result<(), CratestackError> {
        self.validate_at(&FieldPath::Root)
    }
}
