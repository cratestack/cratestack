//! Reserved-identifier rejection for procedure names and procedure
//! argument names — the two `.cstack` ident sites `procedure/types.rs`
//! feeds unguarded into `cratestack_macros::shared::ident` at codegen time
//! (`ident(&to_snake_case(&procedure.name))`, `ident(&arg.name)`). Split
//! out of `mod.rs` to keep that file under the crate's ~200-LoC
//! convention; see [`super::reserved_idents::validate_reserved_identifier`]
//! for the shared check itself.

use cratestack_core::Procedure;

use crate::diagnostics::SchemaError;
use crate::validate::builder_setter_collisions::validate_no_build_setter_collision;
use crate::validate::reserved_idents::validate_reserved_identifier;

pub(super) fn validate_procedure_idents(procedure: &Procedure) -> Result<(), SchemaError> {
    validate_reserved_identifier(
        &procedure.name,
        procedure.name_span,
        &format!("procedure `{}`", procedure.name),
    )?;
    for arg in &procedure.args {
        validate_reserved_identifier(
            &arg.name,
            arg.name_span,
            &format!(
                "procedure argument `{}` on procedure `{}`",
                arg.name, procedure.name
            ),
        )?;
    }
    // Procedure `Args` are struct-shaped and builder-backed too (see
    // `cratestack-macros/src/procedure/types.rs`) — same `build`/
    // `set_build` collision as any other builder-backed field set.
    validate_no_build_setter_collision(
        procedure
            .args
            .iter()
            .map(|arg| (arg.name.as_str(), arg.name_span)),
        "procedure",
        &procedure.name,
    )?;
    Ok(())
}
