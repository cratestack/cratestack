use std::fs;
use std::path::Path;

use cratestack_core::TransportStyle;
use minijinja::Environment;

use crate::config::DartGeneratorError;
use crate::templates_fragments::FRAGMENT_TEMPLATES;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TemplateSpec {
    pub(crate) template_name: &'static str,
    pub(crate) output_path: &'static str,
    pub(crate) default_source: &'static str,
}

// Common templates emitted regardless of the schema's transport.
const COMMON_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "pubspec.yaml.j2",
        output_path: "pubspec.yaml",
        default_source: include_str!("../templates/pubspec.yaml.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: "README.md",
        default_source: include_str!("../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "CHANGELOG.md.j2",
        output_path: "CHANGELOG.md",
        default_source: include_str!("../templates/CHANGELOG.md.j2"),
    },
    TemplateSpec {
        template_name: "analysis_options.yaml.j2",
        output_path: "analysis_options.yaml",
        default_source: include_str!("../templates/analysis_options.yaml.j2"),
    },
    TemplateSpec {
        template_name: "constants.dart.j2",
        output_path: "lib/src/constants.dart",
        default_source: include_str!("../templates/constants.dart.j2"),
    },
    TemplateSpec {
        template_name: "models.dart.j2",
        output_path: "lib/src/models.dart",
        default_source: include_str!("../templates/models.dart.j2"),
    },
    TemplateSpec {
        template_name: "example_main.dart.j2",
        output_path: "example/main.dart",
        default_source: include_str!("../templates/example_main.dart.j2"),
    },
    TemplateSpec {
        template_name: "package_test.dart.j2",
        output_path: "test/{{ package_name }}_test.dart",
        default_source: include_str!("../templates/package_test.dart.j2"),
    },
];

// REST-specific templates. Includes `queries.dart` with selection /
// projection / fetch-query helpers — none of that is useful for RPC
// mode since every call carries a typed body and no URL query.
const REST_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-library.dart.j2",
        output_path: "lib/{{ package_name }}.dart",
        default_source: include_str!("../templates/rest-library.dart.j2"),
    },
    TemplateSpec {
        template_name: "rest-runtime.dart.j2",
        output_path: "lib/src/runtime.dart",
        default_source: include_str!("../templates/rest-runtime.dart.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.dart.j2",
        output_path: "lib/src/queries.dart",
        default_source: include_str!("../templates/rest-queries.dart.j2"),
    },
    TemplateSpec {
        template_name: "rest-apis.dart.j2",
        output_path: "lib/src/apis.dart",
        default_source: include_str!("../templates/rest-apis.dart.j2"),
    },
];

const RPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-library.dart.j2",
        output_path: "lib/{{ package_name }}.dart",
        default_source: include_str!("../templates/rpc-library.dart.j2"),
    },
    TemplateSpec {
        template_name: "rpc-runtime.dart.j2",
        output_path: "lib/src/runtime.dart",
        default_source: include_str!("../templates/rpc-runtime.dart.j2"),
    },
    TemplateSpec {
        template_name: "rpc-apis.dart.j2",
        output_path: "lib/src/apis.dart",
        default_source: include_str!("../templates/rpc-apis.dart.j2"),
    },
];

pub(crate) fn template_specs_for(transport: TransportStyle) -> Vec<TemplateSpec> {
    let mode_specs = match transport {
        TransportStyle::Rest => REST_TEMPLATE_SPECS,
        TransportStyle::Rpc => RPC_TEMPLATE_SPECS,
    };
    let mut specs = Vec::with_capacity(COMMON_TEMPLATE_SPECS.len() + mode_specs.len());
    specs.extend_from_slice(COMMON_TEMPLATE_SPECS);
    specs.extend_from_slice(mode_specs);
    specs
}

pub(crate) fn build_environment(
    template_dir: Option<&Path>,
    specs: &[TemplateSpec],
) -> Result<Environment<'static>, DartGeneratorError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    // {% include %} strips the included template's final newline by
    // default, which breaks byte-identical output when a large template
    // is split into fragments along section boundaries. Preserving the
    // trailing newline keeps the rendered concatenation predictable and
    // matches POSIX text-file convention (files end with \n).
    environment.set_keep_trailing_newline(true);

    for (name, source) in FRAGMENT_TEMPLATES {
        environment
            .add_template(name, source)
            .map_err(|error| DartGeneratorError::TemplateRegistration(name, error))?;
    }

    for spec in specs {
        let source = load_template_source(template_dir, spec)?;
        environment
            .add_template_owned(spec.template_name.to_owned(), source)
            .map_err(|error| DartGeneratorError::TemplateRegistration(spec.template_name, error))?;
    }

    Ok(environment)
}

fn load_template_source(
    template_dir: Option<&Path>,
    spec: &TemplateSpec,
) -> Result<String, DartGeneratorError> {
    let Some(template_dir) = template_dir else {
        return Ok(spec.default_source.to_owned());
    };
    let path = template_dir.join(spec.template_name);
    if !path.exists() {
        return Ok(spec.default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| DartGeneratorError::TemplateRead {
        path: path.display().to_string(),
        template_name: spec.template_name,
        source,
    })
}
