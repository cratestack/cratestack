//! Builds one `lib/src/models/<model>.dart` render context per model —
//! the fan-out half of issue #301: the model's own types (itself,
//! `Create<M>Input`, `Update<M>Input`), any nested `type`/`enum` the
//! partition (`crate::riverpod::partition`) assigned exclusively to this
//! model, its `ProjectedX` view, its `XApi` client class, and its
//! `Provider<XApi>` — all relocated verbatim from today's
//! `rest-apis.dart.j2`/`rpc-apis.dart.j2` per-model loop, not redesigned.
//! `Selection`/`IncludeSelection` (REST only) stay in `queries.dart`
//! instead — see `crate::riverpod::views::QueriesFileContext`'s doc for
//! why (a real cross-file Dart privacy bug, not a style choice).
use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{EnumDecl, Model, Schema, TypeDecl};

use crate::builders::{build_data_class, build_enum_view};
use crate::builders_model::{build_model_accessor, build_model_api, build_selection_model};
use crate::idents::to_camel_case;
use crate::naming::{is_generated_on_create, is_primary_key, model_name_set, scalar_model_fields};
use crate::riverpod::imports::{
    direct_model_refs, model_file_path, model_file_stem, model_relation_targets,
    render_import_lines,
};
use crate::riverpod::partition::{Owner, TypePartition};
use crate::riverpod::provider_naming::reserve_operation_symbol;
use crate::riverpod::views::{ModelFileContext, ModelOperationsView};
use crate::views::DataClassKind;

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_model_file(
    schema: &Schema,
    model: &Model,
    partition: &TypePartition,
    type_by_name: &BTreeMap<&str, &TypeDecl>,
    enum_by_name: &BTreeMap<&str, &EnumDecl>,
    provider_prefix: &str,
    client_class_name: &str,
    is_rest: bool,
    occupied_provider_symbols: &mut BTreeSet<String>,
) -> (String, ModelFileContext) {
    let model_names = model_name_set(&schema.models);
    let enum_names: BTreeSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();

    let model_fields = model.fields.iter().collect::<Vec<_>>();
    let scalar_fields = scalar_model_fields(model, &model_names);

    let mut data_classes = vec![build_data_class(
        &model.name,
        &model_fields,
        DataClassKind::ProjectionModel,
        &enum_names,
    )];

    let create_fields = scalar_fields
        .iter()
        .copied()
        .filter(|field| !is_generated_on_create(field))
        .collect::<Vec<_>>();
    data_classes.push(build_data_class(
        &format!("Create{}Input", model.name),
        &create_fields,
        DataClassKind::Plain,
        &enum_names,
    ));

    let update_fields = scalar_fields
        .iter()
        .copied()
        .filter(|field| !is_primary_key(field))
        .collect::<Vec<_>>();
    data_classes.push(build_data_class(
        &format!("Update{}Input", model.name),
        &update_fields,
        DataClassKind::Patch,
        &enum_names,
    ));

    let locus = Owner::Model(model.name.clone());
    let mut owned_type_decls = Vec::new();
    for name in partition.owned_names(&locus) {
        if let Some(type_decl) = type_by_name.get(name) {
            owned_type_decls.push(*type_decl);
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
        .filter_map(|name| enum_by_name.get(name))
        .map(|enum_decl| build_enum_view(enum_decl))
        .collect();

    let selection = build_selection_model(model, &schema.models, &model_names, &enum_names);
    let model_api = build_model_api(model);
    let accessor = build_model_accessor(model, provider_prefix);

    let operations = ModelOperationsView {
        get_function_name: reserve_operation_symbol(
            &to_camel_case(&model.name),
            false,
            provider_prefix,
            occupied_provider_symbols,
        ),
        list_function_name: reserve_operation_symbol(
            &format!("{}List", to_camel_case(&model.name)),
            false,
            provider_prefix,
            occupied_provider_symbols,
        ),
        create_controller_name: reserve_operation_symbol(
            &format!("{}CreateController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
        update_controller_name: reserve_operation_symbol(
            &format!("{}UpdateController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
        delete_controller_name: reserve_operation_symbol(
            &format!("{}DeleteController", model.name),
            true,
            provider_prefix,
            occupied_provider_symbols,
        ),
    };

    let mut imports: BTreeSet<String> = BTreeSet::new();
    imports.insert("import 'package:flutter_riverpod/flutter_riverpod.dart';".to_owned());
    imports.insert("import 'package:riverpod_annotation/riverpod_annotation.dart';".to_owned());
    imports.insert("import 'package:dart_mappable/dart_mappable.dart';".to_owned());
    imports.insert("import '../runtime.dart';".to_owned());
    imports.insert("import '../client.dart';".to_owned());
    if is_rest {
        imports.insert("import '../queries.dart';".to_owned());
    }
    // `shared_types.dart` also carries `Page`/`PageInfo` (see
    // `build_shared_types`'s doc) — a paged model's own `list()` return
    // type needs it even when the partition found nothing else to share.
    if model_api.is_paged || !partition.shared_refs(&locus).is_empty() {
        imports.insert("import 'shared_types.dart';".to_owned());
    }

    let mut related_models = model_relation_targets(model, &model_names);
    for type_decl in &owned_type_decls {
        related_models.extend(direct_model_refs(type_decl.fields.iter(), &model_names));
    }
    for other in related_models {
        imports.insert(format!("import '{}';", model_file_path(&other)));
    }

    let context = ModelFileContext {
        client_class_name: client_class_name.to_owned(),
        provider_prefix: provider_prefix.to_owned(),
        imports: render_import_lines(imports),
        part_file_name: format!("{}.g.dart", model_file_stem(&model.name)),
        mapper_part_file_name: format!("{}.mapper.dart", model_file_stem(&model.name)),
        enum_types,
        data_classes,
        selection,
        model_api,
        accessor,
        operations,
    };

    (model_file_path(&model.name), context)
}
