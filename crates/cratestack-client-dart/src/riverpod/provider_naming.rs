//! Collision-safe Dart identifiers for issue #302's per-operation
//! `@riverpod` providers.
//!
//! `crate::riverpod::build_library` exports every per-model file *flatly*
//! from the package's barrel (`export 'src/models/<model>.dart';`), so two
//! generated top-level symbols with the same name — even declared in two
//! different model files — become a real `dart analyze` "ambiguous
//! export" error the moment a consumer imports the barrel. A naive
//! `{camelCase(model.name)}{OpWord}` scheme is not collision-free by
//! construction: e.g. model `Widget`'s `list` provider and model
//! `WidgetList`'s `get` provider both want the base name `widgetList`
//! (verified by `riverpod_provider_collision.cstack`, which deliberately
//! constructs this exact case, and its own model/procedure-controller
//! echo). Rather than try to prove a naming scheme collision-free for
//! every possible schema, this module detects a collision when it
//! actually happens and escalates — the same strategy
//! `crate::naming::procedure_wrapper_name` already uses for `{Name}Args`
//! wrapper types.
use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::idents::{to_camel_case, to_pascal_case};
use crate::naming::occupied_type_names;

/// Every top-level Dart symbol the riverpod preset's *other* generated
/// code already declares, before this story's operation providers are
/// assigned any names — the starting point `reserve_operation_symbol`
/// checks new candidates against. Model/type/enum/input names come from
/// `crate::naming::occupied_type_names` (shared with the `default`
/// preset); the rest (`XApi`, `Projected<X>`, `<X>Selection`,
/// `<X>IncludeSelection`, and every DI provider `rest-apis.dart.j2`/
/// `rpc-apis.dart.j2` already emit) is riverpod-preset-specific and not
/// tracked anywhere else, so it's rebuilt here from the same builders
/// (`crate::builders_model`) that actually render it.
pub(crate) fn seed_occupied_symbols(
    schema: &Schema,
    provider_prefix: &str,
    is_rest: bool,
) -> BTreeSet<String> {
    let mut occupied = occupied_type_names(schema);

    for model in &schema.models {
        occupied.insert(format!("{}Api", model.name));
        occupied.insert(format!("Projected{}", model.name));
        occupied.insert(format!("{}Selection", model.name));
        occupied.insert(format!("{}IncludeSelection", model.name));
        occupied.insert(format!("{provider_prefix}{}ApiProvider", model.name));
    }
    occupied.insert(format!("{provider_prefix}AdapterProvider"));
    occupied.insert(format!("{provider_prefix}ClientProvider"));
    occupied.insert(format!("{provider_prefix}ProceduresApiProvider"));
    occupied.insert("ProceduresApi".to_owned());
    if is_rest {
        occupied.insert(format!("{provider_prefix}BasePathProvider"));
    }

    let occupied_types = occupied_type_names(schema);
    for procedure in &schema.procedures {
        occupied.insert(crate::naming::procedure_wrapper_name(
            procedure,
            &occupied_types,
        ));
    }

    occupied
}

/// Reserves a collision-free base identifier for one new `@riverpod`
/// symbol — a function name for a read provider (`widget`, `widgetList`)
/// or a class name for a write controller (`WidgetCreateController`) —
/// and returns it.
///
/// Checks *both* the base identifier and the `...Provider` variable name
/// `riverpod_generator` derives from it (a function keeps its name
/// verbatim plus the `Provider` suffix; a class lower-cases only its
/// first character) against `occupied`, because the two forms don't
/// share a case convention: a base name that's free on its own can still
/// produce an already-taken `...Provider` name against a *different*
/// symbol's own base form. Both forms are inserted into `occupied` the
/// moment a candidate is accepted, so later calls in the same pass (and
/// callers iterating models/procedures in schema order) see it as taken.
///
/// On collision, escalates to `{provider_prefix}{PascalCase(preferred)}`
/// (mirroring `{{ provider_prefix }}AdapterProvider`'s existing
/// schema-wide-prefix convention), then to that qualified name with a
/// growing numeric suffix — `occupied` is finite, so this always
/// terminates.
pub(crate) fn reserve_operation_symbol(
    preferred: &str,
    is_class: bool,
    provider_prefix: &str,
    occupied: &mut BTreeSet<String>,
) -> String {
    let qualified = if is_class {
        format!("{}{preferred}", to_pascal_case(provider_prefix))
    } else {
        format!("{provider_prefix}{}", to_pascal_case(preferred))
    };

    let mut attempt: u32 = 0;
    loop {
        let candidate = match attempt {
            0 => preferred.to_owned(),
            1 => qualified.clone(),
            n => format!("{qualified}{n}"),
        };
        let provider_name = if is_class {
            format!("{}Provider", to_camel_case(&candidate))
        } else {
            format!("{candidate}Provider")
        };
        if !occupied.contains(&candidate) && !occupied.contains(&provider_name) {
            occupied.insert(candidate.clone());
            occupied.insert(provider_name);
            return candidate;
        }
        attempt += 1;
    }
}

#[cfg(test)]
#[path = "provider_naming_tests.rs"]
mod tests;
