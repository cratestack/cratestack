//! Environment + template registry for the `riverpod` preset's fan-out
//! templates (`templates/riverpod/*.j2`). Deliberately separate from
//! `crate::templates` (which the `default` preset still uses, untouched)
//! so nothing here can affect the default preset's byte-identical output
//! contract.
use std::fs;
use std::path::Path;

use minijinja::Environment;

use crate::config::DartGeneratorError;

pub(crate) const REST_MODEL: &str = "riverpod/rest_model.dart.j2";
pub(crate) const RPC_MODEL: &str = "riverpod/rpc_model.dart.j2";
pub(crate) const SHARED_TYPES: &str = "riverpod/shared_types.dart.j2";
pub(crate) const REST_CLIENT: &str = "riverpod/rest_client.dart.j2";
pub(crate) const RPC_CLIENT: &str = "riverpod/rpc_client.dart.j2";
pub(crate) const REST_PROCEDURES: &str = "riverpod/rest_procedures.dart.j2";
pub(crate) const RPC_PROCEDURES: &str = "riverpod/rpc_procedures.dart.j2";
pub(crate) const QUERIES: &str = "riverpod/queries.dart.j2";
pub(crate) const LIBRARY: &str = "riverpod/library.dart.j2";
pub(crate) const PUBSPEC: &str = "riverpod/pubspec.yaml.j2";
pub(crate) const REST_PACKAGE_TEST: &str = "riverpod/rest_package_test.dart.j2";
pub(crate) const RPC_PACKAGE_TEST: &str = "riverpod/rpc_package_test.dart.j2";

/// `(template_name, default_source)`. The first two entries are
/// include-only (issue #301's `enums_and_data_classes.dart.j2` and issue
/// #302's `model_providers.dart.j2`/`procedure_providers.dart.j2`) —
/// registered so `{% include %}` resolves, never rendered to disk
/// directly, mirroring `crate::templates_fragments`.
const TEMPLATE_SOURCES: &[(&str, &str)] = &[
    (
        "model_builder_class.dart.j2",
        include_str!("../../templates/model_builder_class.dart.j2"),
    ),
    (
        "computed_params_class.dart.j2",
        include_str!("../../templates/computed_params_class.dart.j2"),
    ),
    (
        "riverpod/enums_and_data_classes.dart.j2",
        include_str!("../../templates/riverpod/enums_and_data_classes.dart.j2"),
    ),
    (
        "riverpod/model_providers.dart.j2",
        include_str!("../../templates/riverpod/model_providers.dart.j2"),
    ),
    (
        "riverpod/procedure_providers.dart.j2",
        include_str!("../../templates/riverpod/procedure_providers.dart.j2"),
    ),
    (
        REST_MODEL,
        include_str!("../../templates/riverpod/rest_model.dart.j2"),
    ),
    (
        RPC_MODEL,
        include_str!("../../templates/riverpod/rpc_model.dart.j2"),
    ),
    (
        SHARED_TYPES,
        include_str!("../../templates/riverpod/shared_types.dart.j2"),
    ),
    (
        REST_CLIENT,
        include_str!("../../templates/riverpod/rest_client.dart.j2"),
    ),
    (
        RPC_CLIENT,
        include_str!("../../templates/riverpod/rpc_client.dart.j2"),
    ),
    (
        REST_PROCEDURES,
        include_str!("../../templates/riverpod/rest_procedures.dart.j2"),
    ),
    (
        RPC_PROCEDURES,
        include_str!("../../templates/riverpod/rpc_procedures.dart.j2"),
    ),
    (
        QUERIES,
        include_str!("../../templates/riverpod/queries.dart.j2"),
    ),
    (
        LIBRARY,
        include_str!("../../templates/riverpod/library.dart.j2"),
    ),
    (
        PUBSPEC,
        include_str!("../../templates/riverpod/pubspec.yaml.j2"),
    ),
    (
        REST_PACKAGE_TEST,
        include_str!("../../templates/riverpod/rest_package_test.dart.j2"),
    ),
    (
        RPC_PACKAGE_TEST,
        include_str!("../../templates/riverpod/rpc_package_test.dart.j2"),
    ),
];

pub(crate) fn build_environment(
    template_dir: Option<&Path>,
) -> Result<Environment<'static>, DartGeneratorError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    environment.set_keep_trailing_newline(true);

    for &(name, default_source) in TEMPLATE_SOURCES {
        let source = load_template_source(template_dir, name, default_source)?;
        environment
            .add_template_owned(name.to_owned(), source)
            .map_err(|error| DartGeneratorError::TemplateRegistration(name, error))?;
    }

    Ok(environment)
}

fn load_template_source(
    template_dir: Option<&Path>,
    template_name: &'static str,
    default_source: &str,
) -> Result<String, DartGeneratorError> {
    let Some(template_dir) = template_dir else {
        return Ok(default_source.to_owned());
    };
    let path = template_dir.join(template_name);
    if !path.exists() {
        return Ok(default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| DartGeneratorError::TemplateRead {
        path: path.display().to_string(),
        template_name,
        source,
    })
}
