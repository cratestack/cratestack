use cratestack_core::TransportStyle;

use crate::error::TypeScriptGeneratorError;

use super::{OutputPath, TemplateSpec, rpc_specs::RPC_TEMPLATE_SPECS};

// Common templates emitted for both REST and RPC schemas.
pub(crate) const COMMON_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "package.json.j2",
        output_path: OutputPath::Fixed("package.json"),
        default_source: include_str!("../../templates/package.json.j2"),
    },
    TemplateSpec {
        template_name: "tsconfig.json.j2",
        output_path: OutputPath::Fixed("tsconfig.json"),
        default_source: include_str!("../../templates/tsconfig.json.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: OutputPath::Fixed("README.md"),
        default_source: include_str!("../../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "models.ts.j2",
        output_path: OutputPath::Fixed("src/models.ts"),
        default_source: include_str!("../../templates/src/models.ts.j2"),
    },
];

// REST-specific templates. Used when `schema.transport == Rest`.
pub(crate) const REST_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rest-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rest-runtime.ts.j2"),
    },
    // Opt-in compact wire keys (`compactKeys`), imported by the runtime.
    TemplateSpec {
        template_name: "rest-compact.ts.j2",
        output_path: OutputPath::Fixed("src/compact.ts"),
        default_source: include_str!("../../templates/src/rest-compact.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-queries.ts.j2",
        output_path: OutputPath::Fixed("src/queries.ts"),
        default_source: include_str!("../../templates/src/rest-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-client.ts.j2",
        output_path: OutputPath::Fixed("src/client.ts"),
        default_source: include_str!("../../templates/src/rest-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rest-index.ts.j2"),
    },
];

// Issue #617's `--tanstack` opt-in: one extra file, appended to whichever
// mode's spec list only when `TypeScriptGeneratorConfig::tanstack` is set.
// TanStack Query has two hand-written templates (`rest-react-query.ts.j2`,
// `rpc-react-query.ts.j2`) because the generated hooks call different
// runtime APIs per transport, so `template_specs_for` picks one of these
// rather than a single const. They stay out of the unconditional lists so
// `react-query.ts` does not appear in a default run (`tests/snapshot.rs`
// pins that).
const REST_TANSTACK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rest-react-query.ts.j2",
    output_path: OutputPath::Fixed("src/react-query.ts"),
    default_source: include_str!("../../templates/src/rest-react-query.ts.j2"),
}];
const RPC_TANSTACK_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "rpc-react-query.ts.j2",
    output_path: OutputPath::Fixed("src/react-query.ts"),
    default_source: include_str!("../../templates/src/rpc-react-query.ts.j2"),
}];

// Issue #571's `--refine` opt-in: one extra file (the template picks
// `ResourceMap` vs `RpcResourceMap` per transport), kept out of the
// unconditional lists so `refine.ts` does not appear in a default run.
pub(crate) const REFINE_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "refine.ts.j2",
    output_path: OutputPath::Fixed("src/refine.ts"),
    default_source: include_str!("../../templates/src/refine.ts.j2"),
}];

/// Pick the right template specs for the schema's declared transport.
/// REST schemas get the historical fetch-based client + the
/// `CratestackFetchQuery` helpers; RPC schemas get a CratestackRpcRuntime
/// that speaks the `/rpc/{op_id}` URL space, plus their own `queries.ts`
/// (issue #333) — `CratestackRpcListQuery`/`toRpcListInput`, the RPC
/// counterpart of `CratestackFetchQuery`/`toSearchQuery`. RPC's version
/// builds a plain object for the codec-encoded POST body rather than a
/// URL query string (no URL-query shaping needed when every call is a
/// POST with a typed body), but both transports now have a real typed
/// `list` input.
///
/// `tanstack` (issue #617) appends one extra spec, transport-resolved from
/// `REST_TANSTACK_TEMPLATE_SPECS`/`RPC_TANSTACK_TEMPLATE_SPECS` above; it
/// composes with every transport. `refine` (issue #571) appends one extra
/// spec for REST or RPC schemas. Both are parameters rather than folded
/// into `mode_specs` because they are additive to an otherwise unchanged
/// run, and `tanstack`/`rtk` (issue #906, `crate::rtk::specs`) follow the
/// same shape.
pub(crate) fn template_specs_for(
    transport: TransportStyle,
    refine: bool,
    tanstack: bool,
    rtk: bool,
) -> Result<Vec<TemplateSpec>, TypeScriptGeneratorError> {
    let mode_specs = match transport {
        TransportStyle::Rest => REST_TEMPLATE_SPECS,
        TransportStyle::Rpc => RPC_TEMPLATE_SPECS,
    };
    let tanstack_specs: &[TemplateSpec] = if tanstack {
        match transport {
            TransportStyle::Rest => REST_TANSTACK_TEMPLATE_SPECS,
            TransportStyle::Rpc => RPC_TANSTACK_TEMPLATE_SPECS,
        }
    } else {
        &[]
    };
    let rtk_specs = crate::rtk::specs::rtk_specs_for(transport, rtk);
    let refine_specs = if refine { REFINE_TEMPLATE_SPECS } else { &[] };
    Ok([
        COMMON_TEMPLATE_SPECS,
        mode_specs,
        tanstack_specs,
        rtk_specs,
        refine_specs,
    ]
    .concat())
}
