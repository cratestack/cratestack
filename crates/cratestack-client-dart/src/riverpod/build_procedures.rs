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
use crate::dart_types::dart_type;
use crate::idents::to_pascal_case;
use crate::naming::{enum_name_set, model_name_set, occupied_type_names, procedure_wrapper_name};
use crate::riverpod::imports::{model_file_path, owned_type_decl_model_refs, render_import_lines};
use crate::riverpod::partition::{Owner, TypePartition, referenced_name};
use crate::riverpod::provider_naming::reserve_operation_symbol;
use crate::riverpod::views::{ProcedureOperationView, ProceduresFileContext};
use crate::views::DataClassKind;

pub(crate) fn build_procedures_file(
    schema: &Schema,
    partition: &TypePartition,
    provider_prefix: &str,
    client_class_name: &str,
    occupied_provider_symbols: &mut BTreeSet<String>,
) -> ProceduresFileContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied = occupied_type_names(schema);

    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied, &enum_names))
        .collect::<Vec<_>>();

    // Issue #302: one `@riverpod` provider per procedure — a function for
    // `query`-kind procedures (mirrors `ModelOperationsView::get_function_name`/
    // `list_function_name`), a controller class for `mutation`-kind ones
    // (mirrors `create_controller_name`/etc). `procedures`/`procedure_operations`
    // stay index-parallel (`build_procedure` doesn't know about riverpod
    // naming — see `ProceduresFileContext::procedure_operations`'s doc).
    let procedure_operations = schema
        .procedures
        .iter()
        .zip(&procedures)
        .map(|(procedure, view)| {
            let is_mutation = procedure.kind == cratestack_core::ProcedureKind::Mutation;
            let symbol = if is_mutation {
                reserve_operation_symbol(
                    &format!("{}Controller", to_pascal_case(&procedure.name)),
                    true,
                    provider_prefix,
                    occupied_provider_symbols,
                )
            } else {
                reserve_operation_symbol(
                    &view.method_name,
                    false,
                    provider_prefix,
                    occupied_provider_symbols,
                )
            };
            // See `ProcedureOperationView::mutation_method_name`'s doc —
            // `update` is the one name `_$AsyncClassModifier` (riverpod
            // codegen's own base class) already declares, so a procedure
            // that happens to be named `update` needs a non-colliding
            // method name the same way a model's own update controller
            // does (there: always renamed, since a model always has an
            // `update` operation; here: only when the schema's procedure
            // name actually produces `update`).
            let mutation_method_name = if is_mutation && view.method_name == "update" {
                format!("{}Mutation", view.method_name)
            } else {
                view.method_name.clone()
            };

            ProcedureOperationView {
                kind: view.kind,
                symbol,
                nullable_return_type: dart_type(&procedure.return_type, true),
                mutation_method_name,
            }
        })
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
    let mut owned_type_decls = Vec::new();
    for name in partition.owned_names(&locus) {
        if let Some(type_decl) = schema.types.iter().find(|ty| ty.name == name) {
            let fields = type_decl.fields.iter().collect::<Vec<_>>();
            data_classes.push(build_data_class(
                &type_decl.name,
                &fields,
                DataClassKind::Plain,
                &enum_names,
            ));
            owned_type_decls.push(type_decl);
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
    imports.insert("import 'package:riverpod_annotation/riverpod_annotation.dart';".to_owned());
    // issue #325: only when this file actually declares a
    // `@MappableClass()` — a schema with zero procedures and no
    // procedure-owned nested `type`s has zero `data_classes` here, and an
    // unconditional import would be a real `unused_import`
    // `flutter analyze --fatal-warnings` failure (see the matching
    // `mapper_part_file_name` gate in `rest_procedures.dart.j2`/
    // `rpc_procedures.dart.j2` for the paired part-directive concern).
    if !data_classes.is_empty() {
        imports.insert("import 'package:dart_mappable/dart_mappable.dart';".to_owned());
    }
    imports.insert("import 'runtime.dart';".to_owned());
    imports.insert("import 'client.dart';".to_owned());
    // `shared_types.dart` also carries `Page`/`PageInfo`/`PageInput` (see
    // `build_shared_types`'s doc) — a procedure returning/accepting
    // `Page<T>`, or taking a `PageInput` argument, needs it even when the
    // partition found nothing else to share. A `FindMany<Model>` argument
    // is deliberately NOT included in this check: unlike `Page`/
    // `PageInput`, `procedures.dart`'s own generated code never spells
    // `SortDirection`/the filter class names directly — it only ever
    // references the concrete `<Model>FindMany` class (via
    // `models/<model>.dart`, handled below by `referenced_name`'s
    // `FindMany<T>` unwrap), which itself imports `shared_types.dart` for
    // its own `<Model>Where`/`<Model>OrderByClause` fields. Including it
    // here was a confirmed `unused_import` `flutter analyze` failure when
    // no other procedure has a real `Page`/`PageInput` use.
    let uses_page = schema.procedures.iter().any(|procedure| {
        procedure.return_type.is_page()
            || procedure
                .args
                .iter()
                .any(|arg| arg.ty.is_page() || arg.ty.is_page_input())
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
    // cratestack#626: the scan above only reaches models named directly by
    // a procedure's own args/return type — it never looks at the *fields*
    // of the procedure-owned nested `type` declarations emitted above
    // (`owned_type_decls`), so a procedure-only `type` whose field names a
    // `model` (the same shape issue #137 fixed for `build_model.rs`/
    // `build_shared_types.rs`, both of which call this same helper) had
    // its import silently dropped.
    referenced_models.extend(owned_type_decl_model_refs(owned_type_decls, &model_names));
    for other in referenced_models {
        imports.insert(format!("import 'models/{}';", model_file_path(&other)));
    }

    ProceduresFileContext {
        client_class_name: client_class_name.to_owned(),
        provider_prefix: provider_prefix.to_owned(),
        imports: render_import_lines(imports),
        part_file_name: "procedures.g.dart".to_owned(),
        mapper_part_file_name: "procedures.mapper.dart".to_owned(),
        enum_types,
        data_classes,
        procedures,
        procedure_operations,
    }
}
