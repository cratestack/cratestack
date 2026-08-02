//! Template specs for the `swr` preset (issues #304/#305). A handful of
//! specs reuse the default preset's own compiled-in templates verbatim —
//! `runtime.ts`/`queries.ts`/`links.ts`/`cbor-*.ts`/`stream-terminal.ts`
//! and `tsconfig.json` need no per-preset changes, since they're already
//! model-agnostic (see each `.j2` file's own header comment) — everything
//! else (`package.json`, `README.md`, the per-model file + its sibling
//! hooks file, `shared.ts`, `swr-keys.ts`, `procedures.ts` + its sibling
//! hooks file, `index.ts`) is new and preset-specific. See `super`'s own
//! doc comment for why each model/`procedures.ts` gets a *separate*
//! `.hooks.ts` file rather than the hooks living in the same file as the
//! plain functions.

use cratestack_core::TransportStyle;

use crate::templates::{OutputPath, TemplateSpec};

const COMMON: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "swr-package.json.j2",
        output_path: OutputPath::Fixed("package.json"),
        default_source: include_str!("../../templates/swr-package.json.j2"),
    },
    TemplateSpec {
        template_name: "tsconfig.json.j2",
        output_path: OutputPath::Fixed("tsconfig.json"),
        default_source: include_str!("../../templates/tsconfig.json.j2"),
    },
    TemplateSpec {
        template_name: "swr-README.md.j2",
        output_path: OutputPath::Fixed("README.md"),
        default_source: include_str!("../../templates/swr-README.md.j2"),
    },
    TemplateSpec {
        template_name: "swr-models-shared.ts.j2",
        output_path: OutputPath::Fixed("src/models/shared.ts"),
        default_source: include_str!("../../templates/src/swr/models-shared.ts.j2"),
    },
];

const REST: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rest-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.ts.j2",
        output_path: OutputPath::Fixed("src/queries.ts"),
        default_source: include_str!("../../templates/src/rest-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-models-rest.ts.j2",
        output_path: OutputPath::PerModel(".ts"),
        default_source: include_str!("../../templates/src/swr/models-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-models-hooks-rest.ts.j2",
        output_path: OutputPath::PerModel(".hooks.ts"),
        default_source: include_str!("../../templates/src/swr/models-hooks-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-rest.ts.j2",
        output_path: OutputPath::Fixed("src/procedures.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-hooks-rest.ts.j2",
        output_path: OutputPath::Fixed("src/procedures.hooks.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-hooks-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-keys-rest.ts.j2",
        output_path: OutputPath::Fixed("src/swr-keys.ts"),
        default_source: include_str!("../../templates/src/swr/keys-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-index-rest.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/swr/index-rest.ts.j2"),
    },
];

const RPC: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rpc-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-links.ts.j2",
        output_path: OutputPath::Fixed("src/links.ts"),
        default_source: include_str!("../../templates/src/rpc-links.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-item.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-item.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-item.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-seq.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-seq.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-seq.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-stream-terminal.ts.j2",
        output_path: OutputPath::Fixed("src/stream-terminal.ts"),
        default_source: include_str!("../../templates/src/rpc-stream-terminal.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-models-rpc.ts.j2",
        output_path: OutputPath::PerModel(".ts"),
        default_source: include_str!("../../templates/src/swr/models-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-models-hooks-rpc.ts.j2",
        output_path: OutputPath::PerModel(".hooks.ts"),
        default_source: include_str!("../../templates/src/swr/models-hooks-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/procedures.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-hooks-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/procedures.hooks.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-hooks-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-keys-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/swr-keys.ts"),
        default_source: include_str!("../../templates/src/swr/keys-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-index-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/swr/index-rpc.ts.j2"),
    },
];

/// `transport` is REST or RPC only by the time this is called —
/// `swr::generate` rejects `Grpc` before reaching here (see its own doc
/// comment for why: out of scope for issue #304).
pub(crate) fn swr_template_specs_for(transport: TransportStyle) -> Vec<TemplateSpec> {
    let mode = match transport {
        TransportStyle::Rest => REST,
        TransportStyle::Rpc => RPC,
        TransportStyle::Grpc => &[],
    };
    let mut specs = Vec::with_capacity(COMMON.len() + mode.len());
    specs.extend_from_slice(COMMON);
    specs.extend_from_slice(mode);
    specs
}
