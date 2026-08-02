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
use crate::views::{DataClassKind, ModelApiView};

/// `build_model_api` is shared verbatim with the `default` preset (its
/// output is a byte-identical contract — see `tests/snapshot.rs`), so an
/// unpaged model's `list()` return type/decode can't be forked in there.
/// Riverpod additionally depends on `fast_immutable_collections`, so its
/// own per-model file gets `IList<Model>` instead of `List<Model>` for
/// this one field, computed on top of the shared view rather than inside
/// it — mirrors `build_pubspec.rs`'s "own builder, not a conditional
/// branch in the shared one" precedent. Paged models are untouched here:
/// `Page<T>.items` becomes `IList<T>` separately, in
/// `shared_types.dart.j2`, since `Page` itself doesn't change name.
fn build_riverpod_model_api(model: &Model) -> ModelApiView {
    let mut view = build_model_api(model);
    if !view.is_paged {
        view.list_return_type = format!("IList<{}>", model.name);
        view.list_decode_expr = format!(
            "cratestackAsValueList(body).map((item) => {}.fromWire(cratestackAsValueMap(item))).toIList()",
            model.name
        );
    }
    view
}

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
    let model_api = build_riverpod_model_api(model);
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
    // An unpaged model's own `list()`/`listView()` return `IList<...>`
    // (see `build_riverpod_model_api`'s doc), and a list-arity relation
    // getter does too regardless of whether this model itself is paged —
    // only import the package when this file actually references it, per
    // this module's "only import what's used" rule (`dart analyze
    // --fatal-warnings` fails on an unused import).
    let has_list_relation = selection.relations.iter().any(|relation| relation.is_list);
    if !model_api.is_paged || has_list_relation {
        imports.insert(
            "import 'package:fast_immutable_collections/fast_immutable_collections.dart';"
                .to_owned(),
        );
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
