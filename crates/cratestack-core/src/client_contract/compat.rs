//! The compatibility classifier (cratestack#1123, EXT-14): may a server
//! whose op contract is `new` still accept a message an older client signed
//! under contract `old`?
//!
//! The signature proves a peer signed *these bytes under the old shape*. The
//! server may honour it only if decoding those bytes as the new shape yields
//! exactly what the signer encoded, and the old client can decode the answer
//! it gets back. The rules are conservative by construction: anything not
//! named below, and anything this build cannot read, is `Breaking`.
//!
//! Per op, `old` is the signer's shape:
//!
//! - the transport, key, kind, verb, model, events, procedure name and kind,
//!   the return type (so arity and `Page`-ness) and the procedure's kept
//!   attributes are identical;
//! - **input direction** (old client to new server): an existing argument
//!   keeps its name, its relative order and its type, except that a
//!   required one may become optional; an argument may be added only as an
//!   optional one; removing an argument or field the old side could send is
//!   `Breaking` even though the server would decode the message, because
//!   the signed value would be silently ignored (the structs do not
//!   `deny_unknown_fields`), a meaning the signer never produced;
//! - **output direction** (new server to old client): no retype, no arity
//!   change, no field removed, no enum variant added;
//! - every declaration in the op's closure is judged by the directions it is
//!   reachable in, from the input roots (a procedure's arguments, or the
//!   model of a model op, which is both) and the output roots (its return
//!   type, or the model). A declaration reachable both ways gets both rule
//!   sets, and so does one reachable through a relation.
//! - a declaration's fields: each old field stays, with the same attributes
//!   (so `@default` is never added to or removed from an existing field: a
//!   `@default` field is not in the create input, so the value an old client
//!   sent would be dropped), and the same type, except required becomes
//!   optional when the declaration is input-only. A field may be added if
//!   its type is optional or it carries `@default(...)`; when the
//!   declaration is reachable in the output only, any added field is fine,
//!   since a decoder ignores keys it does not know;
//! - enums keep their variants in order; variants may be appended to an
//!   enum that is input-only.
//!
//! Choices the plan left open, all resolved toward refusing: attributes on
//! an existing field or declaration may not be added, removed or changed;
//! an added field must be optional or defaulted even when it is `@readonly`;
//! enum variants may only be appended, never inserted; a declaration leaving
//! the closure is `Breaking`.

use serde::Deserialize;
use serde_json::Value;

use super::compat_decl;
use super::compat_op;
use super::owned::OpContract;

/// The classifier's answer for one op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The older contract may stay accepted.
    Compatible,
    /// It may not; each reason names a change.
    Breaking(Vec<String>),
}

impl Verdict {
    /// Whether the older contract may stay accepted.
    pub fn is_compatible(&self) -> bool {
        matches!(self, Verdict::Compatible)
    }

    /// The reasons a contract is `Breaking`; empty when compatible.
    pub fn reasons(&self) -> &[String] {
        match self {
            Verdict::Compatible => &[],
            Verdict::Breaking(reasons) => reasons,
        }
    }
}

/// Judge the contract `old` (a canonical op contract, as stored in a lock)
/// against `new` (the schema's current one), both as JSON values of the
/// shape `op_contract_json` prints.
pub fn classify(old: &Value, new: &Value) -> Verdict {
    let read = |which: &str, value: &Value| {
        OpContract::deserialize(value)
            .map_err(|e| format!("the {which} contract is unreadable: {e}"))
    };
    let (old, new) = match (read("locked", old), read("current", new)) {
        (Ok(old), Ok(new)) => (old, new),
        (old, new) => {
            return Verdict::Breaking(old.err().into_iter().chain(new.err()).collect());
        }
    };
    let mut reasons = Vec::new();
    compat_op::op(&old, &new, &mut reasons);
    compat_decl::closure(&old, &new, &mut reasons);
    if reasons.is_empty() {
        Verdict::Compatible
    } else {
        Verdict::Breaking(reasons)
    }
}
