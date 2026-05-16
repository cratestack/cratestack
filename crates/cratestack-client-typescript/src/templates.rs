use std::fs;
use std::path::Path;

use cratestack_core::TransportStyle;
use minijinja::Environment;

// Common templates emitted for both REST and RPC schemas.
pub(crate) const COMMON_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "package.json.j2",
        output_path: "package.json",
        default_source: include_str!("../templates/package.json.j2"),
    },
    TemplateSpec {
        template_name: "tsconfig.json.j2",
        output_path: "tsconfig.json",
        default_source: include_str!("../templates/tsconfig.json.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: "README.md",
        default_source: include_str!("../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "models.ts.j2",
        output_path: "src/models.ts",
        default_source: include_str!("../templates/src/models.ts.j2"),
    },
];

// REST-specific templates. Used when `schema.transport == Rest`.
pub(crate) const REST_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-runtime.ts.j2",
        output_path: "src/runtime.ts",
        default_source: include_str!("../templates/src/rest-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.ts.j2",
        output_path: "src/queries.ts",
        default_source: include_str!("../templates/src/rest-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-client.ts.j2",
        output_path: "src/client.ts",
        default_source: include_str!("../templates/src/rest-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-react-query.ts.j2",
        output_path: "src/react-query.ts",
        default_source: include_str!("../templates/src/rest-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-index.ts.j2",
        output_path: "src/index.ts",
        default_source: include_str!("../templates/src/rest-index.ts.j2"),
    },
];

// RPC-specific templates. Used when `schema.transport == Rpc`.
pub(crate) const RPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-runtime.ts.j2",
        output_path: "src/runtime.ts",
        default_source: include_str!("../templates/src/rpc-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-client.ts.j2",
        output_path: "src/client.ts",
        default_source: include_str!("../templates/src/rpc-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-react-query.ts.j2",
        output_path: "src/react-query.ts",
        default_source: include_str!("../templates/src/rpc-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-index.ts.j2",
        output_path: "src/index.ts",
        default_source: include_str!("../templates/src/rpc-index.ts.j2"),
    },
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct TemplateSpec {
    pub(crate) template_name: &'static str,
    pub(crate) output_path: &'static str,
    pub(crate) default_source: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum TypeScriptGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
}

/// Pick the right template specs for the schema's declared transport.
/// REST schemas get the historical fetch-based client + the
/// `CratestackFetchQuery` helpers; RPC schemas get a CratestackRpcRuntime
/// that speaks the `/rpc/{op_id}` URL space and skip `queries.ts` entirely
/// (no URL-query shaping needed when every call is a POST with a typed
/// body).
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
) -> Result<Environment<'static>, TypeScriptGeneratorError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);

    for spec in specs {
        let source = load_template_source(template_dir, spec)?;
        environment
            .add_template_owned(spec.template_name.to_owned(), source)
            .map_err(|error| {
                TypeScriptGeneratorError::TemplateRegistration(spec.template_name, error)
            })?;
    }

    Ok(environment)
}

fn load_template_source(
    template_dir: Option<&Path>,
    spec: &TemplateSpec,
) -> Result<String, TypeScriptGeneratorError> {
    let Some(template_dir) = template_dir else {
        return Ok(spec.default_source.to_owned());
    };
    let path = template_dir.join(spec.template_name);
    if !path.exists() {
        return Ok(spec.default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| TypeScriptGeneratorError::TemplateRead {
        path: path.display().to_string(),
        template_name: spec.template_name,
        source,
    })
}
