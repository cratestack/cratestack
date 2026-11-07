use super::{OutputPath, TemplateSpec};

// RPC-specific templates. Used when `schema.transport == Rpc`.
pub(super) const RPC_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-runtime.ts.j2",
        output_path: OutputPath::Fixed("src/runtime.ts"),
        default_source: include_str!("../../templates/src/rpc-runtime.ts.j2"),
    },
    // Opt-in compact wire keys (`compactKeys`), imported by the runtime.
    TemplateSpec {
        template_name: "rpc-compact.ts.j2",
        output_path: OutputPath::Fixed("src/compact.ts"),
        default_source: include_str!("../../templates/src/rpc-compact.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-links.ts.j2",
        output_path: OutputPath::Fixed("src/links.ts"),
        default_source: include_str!("../../templates/src/rpc-links.ts.j2"),
    },
    // CBOR argument parsing, structural walking, and stream framing.
    TemplateSpec {
        template_name: "rpc-cbor-argument.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-argument.ts"),
        default_source: include_str!("../../templates/src/rpc-cbor-argument.ts.j2"),
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
        template_name: "rpc-index.ts.j2",
        output_path: OutputPath::Fixed("src/index.ts"),
        default_source: include_str!("../../templates/src/rpc-index.ts.j2"),
    },
];
