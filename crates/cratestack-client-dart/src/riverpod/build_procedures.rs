//! Builds `lib/src/procedures.dart` — `ProceduresApi` and its provider,
//! relocated verbatim from today's `rest-apis.dart.j2`/`rpc-apis.dart.j2`
//! (the `class ProceduresApi { ... }` block was already isolated there,
//! never attached to any one model), plus whatever nested `type`/`enum`
//! the partition assigned to `Owner::Procedures` — always emitted, even
//! for a schema with zero procedures, mirroring the `default` preset's
//! `ProceduresApi` always existing.
use std::collections::BTreeSet;

use cratestack_core::Field;
use cratestack_core::Schema;

use crate::builders::{build_data_class, build_enum_view};
use crate::builders_model::build_procedure;
use crate::naming::{enum_name_set, model_name_set, occupied_type_names, procedure_wrapper_name};
use crate::riverpod::imports::{model_file_path, render_import_lines};
use crate::riverpod::partition::{Owner, TypePartition, referenced_name};
use crate::riverpod::views::ProceduresFileContext;
use crate::views::DataClassKind;

pub(crate) fn build_procedures_file(
    schema: &Schema,
    partition: &TypePartition,
    provider_prefix: &str,
    client_class_name: &str,
) -> ProceduresFileContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied = occupied_type_names(schema);

    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied, &enum_names))
        .collect::<Vec<_>>();

    let mut data_classes = Vec::new();
    for procedure in &schema.procedures {
        let args_name = procedure_wrapper_name(procedure, &occupied);
        let fields = procedure
            .args
            .iter()
            .map(|arg| Field {
                docs: arg.docs.clone(),
                name: arg.name.clone(),
                name_span: arg.name_span,
                ty: arg.ty.clone(),
                attributes: Vec::new(),
                span: procedure.span,
            })
            .collect::<Vec<_>>();
        let field_refs = fields.iter().collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &args_name,
            &field_refs,
            DataClassKind::Plain,
            &enum_names,
        ));
    }

    let locus = Owner::Procedures;
    for name in partition.owned_names(&locus) {
        if let Some(type_decl) = schema.types.iter().find(|ty| ty.name == name) {
            let fields = type_decl.fields.iter().collect::<Vec<_>>();
            data_classes.push(build_data_class(
                &type_decl.name,
                &fields,
                DataClassKind::Plain,
                &enum_names,
            ));
        }
    }

    let enum_types = partition
        .owned_names(&locus)
        .into_iter()
        .filter_map(|name| schema.enums.iter().find(|decl| decl.name == name))
        .map(build_enum_view)
        .collect();

    let mut imports: BTreeSet<String> = BTreeSet::new();
    imports.insert("import 'package:flutter_riverpod/flutter_riverpod.dart';".to_owned());
    imports.insert("import 'runtime.dart';".to_owned());
    imports.insert("import 'client.dart';".to_owned());
    // `shared_types.dart` also carries `Page`/`PageInfo` (see
    // `build_shared_types`'s doc) — a procedure returning/accepting
    // `Page<T>` needs it even when the partition found nothing else to
    // share.
    let uses_page = schema.procedures.iter().any(|procedure| {
        procedure.return_type.is_page() || procedure.args.iter().any(|arg| arg.ty.is_page())
    });
    if uses_page || !partition.shared_refs(&locus).is_empty() {
        imports.insert("import 'models/shared_types.dart';".to_owned());
    }

    let mut referenced_models = BTreeSet::new();
    for procedure in &schema.procedures {
        for arg in &procedure.args {
            let name = referenced_name(&arg.ty);
            if model_names.contains(name.as_str()) {
                referenced_models.insert(name);
            }
        }
        let name = referenced_name(&procedure.return_type);
        if model_names.contains(name.as_str()) {
            referenced_models.insert(name);
        }
    }
    for other in referenced_models {
        imports.insert(format!("import 'models/{}';", model_file_path(&other)));
    }

    ProceduresFileContext {
        client_class_name: client_class_name.to_owned(),
        provider_prefix: provider_prefix.to_owned(),
        imports: render_import_lines(imports),
        enum_types,
        data_classes,
        procedures,
    }
}
