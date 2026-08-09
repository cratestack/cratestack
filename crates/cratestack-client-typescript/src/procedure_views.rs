//! `ProcedureView`/`build_procedure` — split out of `views.rs` (which
//! covers models/enums/interfaces) to keep both files closer to this
//! repo's ~200-LoC convention, mirroring `find_many_views.rs`'s existing
//! split for `Where`/`OrderBy`/`FindMany`.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, ProcedureKind};
use serde::Serialize;

use crate::decimal::{ProcedureDecimalRevival, procedure_decimal_revival};
use crate::naming::{procedure_wrapper_name, to_camel_case, to_pascal_case};
use crate::types::ts_type;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureView {
    pub(crate) name: String,
    pub(crate) method_name: String,
    pub(crate) hook_name: String,
    pub(crate) args_name: String,
    pub(crate) return_type: String,
    pub(crate) route: String,
    pub(crate) kind: &'static str,
    pub(crate) query_key: String,
    pub(crate) mutation_key: String,
    /// `"scalar"` when the return type is (optionally list/optional)
    /// `Decimal` itself — the decoded value is a raw string (or array of
    /// strings, or null), not an object, so the generated call site uses
    /// `reviveDecimalScalar` instead of `reviveDecimalFields`. `"shape"`
    /// for every other return type (cratestack#498 F2): a `Model`/`type`
    /// (optionally `Page<...>`/list/optional-wrapped), an enum, or a plain
    /// scalar other than `Decimal` — `decimal_shape_name` names the
    /// registry entry for that case (a name with no entry, e.g.
    /// `echoName(): String`, is `reviveDecimalFields`'s documented no-op
    /// fast path).
    pub(crate) decimal_revival_kind: &'static str,
    /// Only meaningful when `decimal_revival_kind == "shape"` — the
    /// (`Page<T>`-unwrapped) base return type's name, a registry key into
    /// `models.ts.j2`'s generated `decimalShapes` object. Empty string for
    /// `"scalar"`.
    pub(crate) decimal_shape_name: String,
    /// Only meaningful when `decimal_revival_kind == "shape"` — `true`
    /// when the return type was `Page<T>` (see `views::ModelApiView::
    /// is_paged`'s doc comment for why that needs a different runtime
    /// helper). `false` for `"scalar"`.
    pub(crate) decimal_paged: bool,
}

pub(crate) fn build_procedure(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
    enum_names: &BTreeSet<&str>,
) -> ProcedureView {
    let (decimal_revival_kind, decimal_shape_name, decimal_paged) =
        match procedure_decimal_revival(&procedure.return_type) {
            ProcedureDecimalRevival::Scalar => ("scalar", String::new(), false),
            ProcedureDecimalRevival::Shape { shape_name, paged } => ("shape", shape_name, paged),
        };
    ProcedureView {
        name: procedure.name.clone(),
        method_name: to_camel_case(&procedure.name),
        hook_name: to_pascal_case(&procedure.name),
        args_name: procedure_wrapper_name(procedure, occupied_type_names),
        return_type: ts_type(&procedure.return_type, enum_names),
        route: format!("/$procs/{}", procedure.name),
        kind: match procedure.kind {
            ProcedureKind::Query => "query",
            ProcedureKind::Mutation => "mutation",
        },
        query_key: format!("{}Procedure", to_camel_case(&procedure.name)),
        mutation_key: format!("{}Procedure", to_camel_case(&procedure.name)),
        decimal_revival_kind,
        decimal_shape_name,
        decimal_paged,
    }
}
