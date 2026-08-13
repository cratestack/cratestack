use cratestack_core::TransportStyle;

use crate::error::TypeScriptGeneratorError;

use super::{OutputPath, TemplateSpec};

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
        template_name: "rest-react-query.ts.j2",
        output_path: OutputPath::Fixed("src/react-query.ts"),
        default_source: include_str!("../../templates/src/rest-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "rest-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rest-index.ts.j2"),
    },
];

// RPC-specific templates. Used when `schema.transport == Rpc`.
pub(crate) const RPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
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
    // Issue #277's `application/cbor-seq` boundary scanner, split across
    // two files by concern (see each file's own header comment): the
    // low-level single-item structural walk, and the stateful
    // chunk-buffering scanner + error-sentinel classification built on
    // it. Both stay under this repo's ~200-LoC convention individually;
    // a single merged file wouldn't have.
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
    // The `streamLinks` chain's terminal link (issue #277) — split out
    // of `rpc-runtime.ts.j2` to avoid growing that already-over-budget
    // file further; see its own header comment.
    TemplateSpec {
        template_name: "rpc-stream-terminal.ts.j2",
        output_path: OutputPath::Fixed("src/stream-terminal.ts"),
        default_source: include_str!("../../templates/src/rpc-stream-terminal.ts.j2"),
    },
    // Typed `model.<X>.list` query builder (issue #333) — mirrors
    // `rest-queries.ts.j2`'s position ahead of `rest-client.ts.j2` above:
    // the client template imports `toRpcListInput`/`CratestackRpcListQuery`
    // from here.
    TemplateSpec {
        template_name: "rpc-queries.ts.j2",
        output_path: OutputPath::Fixed("src/queries.ts"),
        default_source: include_str!("../../templates/src/rpc-queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-client.ts.j2",
        output_path: OutputPath::Fixed("src/client.ts"),
        default_source: include_str!("../../templates/src/rpc-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-react-query.ts.j2",
        output_path: OutputPath::Fixed("src/react-query.ts"),
        default_source: include_str!("../../templates/src/rpc-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rpc-index.ts.j2"),
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
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/grpc-web-runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-client.ts.j2",
        output_path: OutputPath::Fixed("src/client.ts"),
        default_source: include_str!("../../templates/src/grpc-web-client.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-react-query.ts.j2",
        output_path: OutputPath::Fixed("src/react-query.ts"),
        default_source: include_str!("../../templates/src/grpc-web-react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "grpc-web-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/grpc-web-index.ts.j2"),
    },
];

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
