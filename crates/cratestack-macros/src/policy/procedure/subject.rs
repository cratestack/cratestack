//! What a procedure-dialect policy expression is being resolved *against*.
//!
//! The dialect is shared by two constructs — `procedure` and, since
//! cratestack#867, `query` — and design §6 chose it for `query`
//! specifically because the resolver reads nothing but a name, an
//! argument list and the schema's `type` declarations. [`PolicySubject`]
//! makes that literal: the resolver no longer takes a `Procedure` it
//! only ever reads three fields of, so a `query` needs no stand-in value
//! to be resolved.
//!
//! It also carries the noun. Before cratestack#867's review, every
//! diagnostic on this path said "procedure" regardless — a schema author
//! who mistyped an argument in a `query`'s `@allow` was told about a
//! procedure they had not written. Threading the word through is the only
//! way to fix that at the source rather than by rewriting strings at the
//! call site.

use cratestack_core::{Procedure, ProcedureArg, Query};

#[derive(Clone, Copy)]
pub(crate) struct PolicySubject<'a> {
    /// The schema-author-facing noun for this construct — `"procedure"`
    /// or `"query"`. Appears in every diagnostic this module produces.
    pub(super) construct: &'static str,
    pub(super) name: &'a str,
    pub(super) args: &'a [ProcedureArg],
}

impl<'a> PolicySubject<'a> {
    pub(crate) fn procedure(procedure: &'a Procedure) -> Self {
        Self {
            construct: "procedure",
            name: &procedure.name,
            args: &procedure.args,
        }
    }

    pub(crate) fn query(query: &'a Query) -> Self {
        Self {
            construct: "query",
            name: &query.name,
            args: &query.args,
        }
    }

    /// "unknown query input field `x` on `totals`", plus the declared
    /// parameter list.
    ///
    /// The list is the point. A bare "unknown input field" leaves the
    /// author comparing their policy against their signature by eye,
    /// which is exactly the mistake that produced the message; printing
    /// what *is* declared turns a typo into a one-glance fix.
    pub(super) fn unknown_field(&self, field: &str) -> String {
        format!(
            "unknown {} input field `{field}` on `{}` ({})",
            self.construct,
            self.name,
            self.declared_parameters(),
        )
    }

    pub(super) fn declared_parameters(&self) -> String {
        if self.args.is_empty() {
            return "it declares no parameters".to_owned();
        }
        let names = self
            .args
            .iter()
            .map(|arg| format!("`{}`", arg.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("declared parameters: {names}")
    }
}
