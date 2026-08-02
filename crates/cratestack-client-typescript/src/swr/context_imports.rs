//! Lower-level helpers `context.rs`'s two builder functions share: the
//! owned/shared type split, the model-reference scans that feed
//! cross-file `import type` lines, and the import-list assembly itself.
//! Split out to keep `context.rs` under this repo's ~200-LoC convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, Procedure, Schema, TypeDecl};

use crate::naming::to_kebab_case;
use crate::types::base_type_name;
use crate::views::{EnumView, InterfaceKind, InterfaceView, build_enum_view, build_interface};

use super::ownership::{TypeOwner, TypeOwnership};
use super::views::SwrImport;

/// Splits every declared enum/`type` into `(enums, interfaces)` whose
/// owner (per `crate::swr::ownership`) matches `predicate` — the one place
/// that turns an owner classification into the two view-lists every swr
/// file (shared/model/procedures) inlines.
pub(super) fn owned_by(
    schema: &Schema,
    ownership: &TypeOwnership,
    enum_names: &BTreeSet<&str>,
    predicate: impl Fn(&TypeOwner) -> bool,
) -> (Vec<EnumView>, Vec<InterfaceView>) {
    let enums = schema
        .enums
        .iter()
        .filter(|e| ownership.owner_of(&e.name).is_some_and(&predicate))
        .map(build_enum_view)
        .collect();
    let interfaces = schema
        .types
        .iter()
        .filter(|ty| ownership.owner_of(&ty.name).is_some_and(&predicate))
        .map(|ty| {
            build_interface(
                &ty.name,
                &ty.fields.iter().collect::<Vec<_>>(),
                InterfaceKind::Plain,
                enum_names,
            )
        })
        .collect();
    (enums, interfaces)
}

/// Model names referenced by a set of fields (a relation, e.g. `author
/// User`) — the cross-model type-only import case, distinct from the
/// enum/`type` ownership computation (see `crate::swr::ownership`'s
/// module doc for why these are handled separately).
pub(super) fn model_refs_in_fields<'a>(
    fields: impl Iterator<Item = &'a Field>,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    fields
        .map(|field| base_type_name(&field.ty))
        .filter(|name| model_names.contains(name))
        .map(str::to_owned)
        .collect()
}

/// A `type` block placed in a file by `predicate` (owned/shared) can still
/// have a field that names a *model* directly (issue #137's
/// `type ApiKeySecret { model SomeModel; ... }` shape) — that model import
/// has to travel with the type wherever it's placed. See
/// `crate::swr::ownership`'s module doc: this can never need a *model*
/// currently owning a *different* eligible type it depends on, only a
/// direct model-typed field on the `type` block itself.
pub(super) fn type_decls_model_refs(
    schema: &Schema,
    ownership: &TypeOwnership,
    model_names: &BTreeSet<&str>,
    predicate: impl Fn(&TypeOwner) -> bool,
) -> BTreeSet<String> {
    schema
        .types
        .iter()
        .filter(|ty| ownership.owner_of(&ty.name).is_some_and(&predicate))
        .flat_map(|ty: &TypeDecl| model_refs_in_fields(ty.fields.iter(), model_names))
        .collect()
}

/// Model names a procedure's args/return type reference directly.
pub(super) fn procedure_model_refs(
    schema: &Schema,
    model_names: &BTreeSet<&str>,
) -> BTreeSet<String> {
    schema
        .procedures
        .iter()
        .flat_map(|procedure| {
            let mut names = BTreeSet::new();
            for arg in &procedure.args {
                let name = base_type_name(&arg.ty);
                if model_names.contains(name) {
                    names.insert(name.to_owned());
                }
            }
            let return_name = base_type_name(&procedure.return_type);
            if model_names.contains(return_name) {
                names.insert(return_name.to_owned());
            }
            names
        })
        .collect()
}

pub(super) fn procedure_arg_fields(procedure: &Procedure) -> Vec<Field> {
    procedure
        .args
        .iter()
        .map(|arg| Field {
            docs: arg.docs.clone(),
            name: arg.name.clone(),
            name_span: arg.name_span,
            ty: arg.ty.clone(),
            attributes: Vec::new(),
            span: arg.span,
        })
        .collect()
}

pub(super) fn build_imports(
    shared_names: Vec<String>,
    model_refs: BTreeSet<String>,
    self_model: Option<&str>,
    shared_path: &str,
    model_path_prefix: &str,
) -> Vec<SwrImport> {
    let mut imports = Vec::new();
    if !shared_names.is_empty() {
        imports.push(SwrImport::new(shared_path.to_owned(), shared_names));
    }
    for name in model_refs {
        if Some(name.as_str()) == self_model {
            continue;
        }
        let path = format!("{model_path_prefix}{}", to_kebab_case(&name));
        imports.push(SwrImport::new(path, vec![name]));
    }
    imports
}
