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

// gRPC-Web-specific templates. Used when `schema.transport == Grpc`.
// Model CRUD only (ticket #172 — see `crate::grpc`'s module doc): no
// `queries.ts` (no URL-query shaping — protobuf fields are typed, not
// query-string-shaped) and no procedure surface (ticket #171 never wired
// procedures into the generated tonic service, so there is nothing to
// bind a method to).
pub(crate) const GRPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "grpc-web-runtime.ts.j2",
        output_path: "src/runtime.ts",
        default_source: include_str!("../templates/src/grpc-web-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-client.ts.j2",
        output_path: "src/client.ts",
        default_source: include_str!("../templates/src/grpc-web-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-react-query.ts.j2",
        output_path: "src/react-query.ts",
        default_source: include_str!("../templates/src/grpc-web-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-index.ts.j2",
        output_path: "src/index.ts",
        default_source: include_str!("../templates/src/grpc-web-index.ts.j2"),
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
    /// `transport grpc` schema, but `TypeScriptGeneratorConfig::pb_lock`
    /// was `None`. The gRPC-Web wire codec needs the real field numbers
    /// `cratestack generate-proto` assigns — run that first (or pass its
    /// output through) before generating a `transport grpc` client.
    #[error(
        "schema declares `transport grpc`, which needs the schema's `.pb.lock` to generate a \
         gRPC-Web client — run `cratestack generate-proto` first and pass its lock via \
         `TypeScriptGeneratorConfig::pb_lock`"
    )]
    MissingPbLock,
    /// The lock parsed, but has no `package` — shouldn't happen for a lock
    /// `generate-proto` produced (`--package` is required on first run,
    /// `docs/design/protobuf.md` §4.6), but a hand-edited or pre-package
    /// lock is possible, so this is a real error rather than a panic.
    #[error(
        "the schema's `.pb.lock` has no `package` set — gRPC-Web method paths need it \
         (`/<package>.Api/<Method>`); re-run `cratestack generate-proto --package <name>`"
    )]
    MissingPbLockPackage,
    /// The lock is missing an entry (message or field) the schema expects
    /// — a stale lock relative to the schema. `cratestack generate-proto
    /// --check` is the tool that catches and reports this drift in detail;
    /// this generator just refuses to guess a field number. `field` is
    /// pre-formatted by the call site (` field \`x\`` or empty) rather than
    /// interpolated here, to keep the `#[error(...)]` format string a
    /// plain literal.
    #[error(
        "`.pb.lock` is missing an entry for message `{message}`{field}: re-run `cratestack generate-proto` to refresh it"
    )]
    MissingPbLockEntry { message: String, field: String },
}

/// Pick the right template specs for the schema's declared transport.
/// REST schemas get the historical fetch-based client + the
/// `CratestackFetchQuery` helpers; RPC schemas get a CratestackRpcRuntime
/// that speaks the `/rpc/{op_id}` URL space and skip `queries.ts` entirely
/// (no URL-query shaping needed when every call is a POST with a typed
/// body).
pub(crate) fn template_specs_for(
    transport: TransportStyle,
) -> Result<Vec<TemplateSpec>, TypeScriptGeneratorError> {
    let mode_specs = match transport {
        TransportStyle::Rest => REST_TEMPLATE_SPECS,
        TransportStyle::Rpc => RPC_TEMPLATE_SPECS,
        TransportStyle::Grpc => GRPC_TEMPLATE_SPECS,
    };
    let mut specs = Vec::with_capacity(COMMON_TEMPLATE_SPECS.len() + mode_specs.len());
    specs.extend_from_slice(COMMON_TEMPLATE_SPECS);
    specs.extend_from_slice(mode_specs);
    Ok(specs)
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
