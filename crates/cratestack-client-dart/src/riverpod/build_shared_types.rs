//! Builds `lib/src/models/shared_types.dart` — always emitted (unlike
//! the other per-locus files, it isn't conditional): it carries the
//! `Page`/`PageInfo` wrapper types every `@@paged` model needs (hardcoded
//! directly in `templates/riverpod/shared_types.dart.j2`, mirroring
//! `models.dart.j2`'s own hardcoded copy), plus every nested `type`/
//! `enum` the partition (`crate::riverpod::partition`) assigned to
//! `Owner::Shared` because more than one locus (or zero) reaches it.
use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::builders::{build_data_class, build_enum_view};
use crate::naming::{enum_name_set, model_name_set};
use crate::riverpod::imports::{model_file_path, owned_type_decl_model_refs, render_import_lines};
use crate::riverpod::partition::{Owner, TypePartition};
use crate::riverpod::views::SharedTypesFileContext;
use crate::views::DataClassKind;

pub(crate) fn build_shared_types_file(
    schema: &Schema,
    partition: &TypePartition,
) -> SharedTypesFileContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);

    let mut data_classes = Vec::new();
    let mut owned_type_decls = Vec::new();
    for ty in &schema.types {
        if *partition.type_owner(&ty.name) != Owner::Shared {
            continue;
        }
        let fields = ty.fields.iter().collect::<Vec<_>>();
        // A `type` block can be genuinely `Owner::Shared` after all — not
        // just via two-or-more-models sharing it (structurally impossible,
        // see `tests/fixtures/riverpod_shared_ownership.cstack`'s doc), but
        // via an ORPHAN `type` referenced by nothing at all: `Owner::
        // owner_by_name` defaults an unreferenced name to `Owner::Shared`
        // (see `tests/fixtures/riverpod_shared_type_orphan.cstack`).
        // `shared_types.dart` gets a builder here like every other
        // `build_data_class` call site — an earlier revision forced
        // `emit_builder` back to `false` on the premise that this file
        // "emits no builders", which origin/main's inline emission
        // disproves (the orphan fixture's baseline `shared_types.dart`
        // does declare `class CoordinatesBuilder`).
        let data_class = build_data_class(
            &ty.name,
            &fields,
            DataClassKind::Plain,
            &enum_names,
            &model_names,
        );
        data_classes.push(data_class);
        owned_type_decls.push(ty);
    }
    let referenced_models = owned_type_decl_model_refs(owned_type_decls, &model_names);

    let enum_types = schema
        .enums
        .iter()
        .filter(|decl| *partition.enum_owner(&decl.name) == Owner::Shared)
        .map(build_enum_view)
        .collect::<Vec<_>>();

    let imports = referenced_models
        .into_iter()
        .map(|other| format!("import '{}';", model_file_path(&other)))
        .collect::<BTreeSet<_>>();

    SharedTypesFileContext {
        imports: render_import_lines(imports),
        enum_types,
        data_classes,
    }
}
