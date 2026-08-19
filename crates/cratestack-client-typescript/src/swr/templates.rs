//! Template specs for the `--swr` flag (issues #304/#305, made additive by
//! #591). A handful of specs reuse the default layout's own compiled-in
//! templates verbatim — `runtime.ts`/`queries.ts`/`links.ts`/
//! `cbor-*.ts`/`stream-terminal.ts` need no per-layout changes, since
//! they're already model-agnostic (see each `.j2` file's own header
//! comment) — everything else (the per-model file + its sibling hooks
//! file, `shared.ts`, `swr-keys.ts`, `procedures.ts` + its sibling hooks
//! file, `index.ts`) is `swr`-specific. `package.json`/`tsconfig.json`/
//! `README.md` are deliberately NOT in this list — issue #591 made
//! `--swr` additive to the default layout rather than a replacement for
//! it, so those three come from the default layout's own copies (always
//! emitted, `--swr` or not) instead of being duplicated here. See
//! `super`'s own doc comment for the full rationale, and for why each
//! model/`procedures.ts` gets a *separate* `.hooks.ts` file rather than
//! the hooks living in the same file as the plain functions.

use cratestack_core::TransportStyle;

use crate::templates::{OutputPath, TemplateSpec};

const COMMON: &[TemplateSpec] = &[TemplateSpec {
    template_name: "swr-models-shared.ts.j2",
    output_path: OutputPath::Fixed("src/swr/models/shared.ts"),
    default_source: include_str!("../../templates/src/swr/models-shared.ts.j2"),
}];

const REST: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/swr/runtime.ts"),
        default_source: include_str!("../../templates/src/rest-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.ts.j2",
        output_path: OutputPath::Fixed("src/swr/queries.ts"),
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
        output_path: OutputPath::Fixed("src/swr/procedures.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-hooks-rest.ts.j2",
        output_path: OutputPath::Fixed("src/swr/procedures.hooks.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-hooks-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-keys-rest.ts.j2",
        output_path: OutputPath::Fixed("src/swr/swr-keys.ts"),
        default_source: include_str!("../../templates/src/swr/keys-rest.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-index-rest.ts.j2",
        output_path: OutputPath::Fixed("src/swr/index.ts"),
        default_source: include_str!("../../templates/src/swr/index-rest.ts.j2"),
    },
];

const RPC: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/swr/runtime.ts"),
        default_source: include_str!("../../templates/src/rpc-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-links.ts.j2",
        output_path: OutputPath::Fixed("src/swr/links.ts"),
        default_source: include_str!("../../templates/src/rpc-links.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-item.ts.j2",
        output_path: OutputPath::Fixed("src/swr/cbor-item.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-item.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-seq.ts.j2",
        output_path: OutputPath::Fixed("src/swr/cbor-seq.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-seq.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-stream-terminal.ts.j2",
        output_path: OutputPath::Fixed("src/swr/stream-terminal.ts"),
        default_source: include_str!("../../templates/src/rpc-stream-terminal.ts.j2"),
    },
    // Typed `model.<X>.list` query builder (issue #333) — reused verbatim
    // from the default layout, same as `rest-queries.ts.j2` is reused for
    // the REST arm above: model-agnostic, so no `swr`-specific variant.
    TemplateSpec {
        template_name: "rpc-queries.ts.j2",
        output_path: OutputPath::Fixed("src/swr/queries.ts"),
        default_source: include_str!("../../templates/src/rpc-queries.ts.j2"),
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
        output_path: OutputPath::Fixed("src/swr/procedures.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-procedures-hooks-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/swr/procedures.hooks.ts"),
        default_source: include_str!("../../templates/src/swr/procedures-hooks-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-keys-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/swr/swr-keys.ts"),
        default_source: include_str!("../../templates/src/swr/keys-rpc.ts.j2"),
    },
    TemplateSpec {
        template_name: "swr-index-rpc.ts.j2",
        output_path: OutputPath::Fixed("src/swr/index.ts"),
        default_source: include_str!("../../templates/src/swr/index-rpc.ts.j2"),
    },
];

pub(crate) fn swr_template_specs_for(transport: TransportStyle) -> Vec<TemplateSpec> {
    let mode = match transport {
        TransportStyle::Rest => REST,
        TransportStyle::Rpc => RPC,
    };
    let mut specs = Vec::with_capacity(COMMON.len() + mode.len());
    specs.extend_from_slice(COMMON);
    specs.extend_from_slice(mode);
    specs
}
