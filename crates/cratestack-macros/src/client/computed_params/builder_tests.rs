//! Builder tests for `<Model>ComputedParams` — kept separate from
//! `tests.rs` solely so concurrent work owns disjoint files.

use std::collections::BTreeSet;

use quote::ToTokens;

use crate::client::generate_client_module;
use cratestack_core::TransportStyle;

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse")
}

fn render(
    schema: &cratestack_core::Schema,
    transport: TransportStyle,
    bearing: &BTreeSet<String>,
) -> String {
    generate_client_module(&schema.models, &schema.procedures, transport, bearing)
        .expect("client module should generate")
        .to_token_stream()
        .to_string()
}

const PARAMETERIZED_SCHEMA: &str = "\
model Photo {\n  id Int @id\n  storageKey String\n  proxyUrl String @computed(params: ProxyParams?)\n}\n\
type ProxyParams {\n  width Int?\n}\n";

const BARE_ONLY_SCHEMA: &str = "\
model BarePhoto {\n  id Int @id\n  storageKey String\n  thumbnailUrl String @computed\n}\n";

const TWO_PARAMETERIZED_FIELDS_SCHEMA: &str = "\
model Video {\n  id Int @id\n  storageKey String\n  proxyUrl String @computed(params: ProxyParams?)\n  captionUrl String @computed(params: CaptionParams?)\n}\n\
type ProxyParams {\n  width Int?\n}\n\
type CaptionParams {\n  locale String?\n}\n";

#[test]
fn computed_params_struct_gets_a_builder() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains("struct PhotoComputedParamsBuilder"),
        "PhotoComputedParams must have a builder struct — {rendered}"
    );
    assert!(
        rendered.contains("pub fn builder () -> PhotoComputedParamsBuilder"),
        "PhotoComputedParams must have a builder() method — {rendered}"
    );
}

#[test]
fn computed_params_builder_is_non_generic() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains("pub struct PhotoComputedParamsBuilder"),
        "PhotoComputedParamsBuilder struct must be emitted — {rendered}"
    );
    // The builder is non-generic (no type parameters), so it should NOT be
    // followed by `<` for type params.
    let builder_decl_start = rendered
        .find("pub struct PhotoComputedParamsBuilder")
        .unwrap();
    let after_decl =
        &rendered[builder_decl_start + "pub struct PhotoComputedParamsBuilder".len()..];
    let first_100_chars = &after_decl[..after_decl.len().min(100)];
    assert!(
        !first_100_chars.starts_with(" <"),
        "PhotoComputedParamsBuilder must not be followed by generic type parameters — {rendered}"
    );
}

#[test]
fn computed_params_builder_setter_takes_the_option() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains(":: core :: option :: Option < super :: types :: ProxyParams >"),
        "the builder setter must take Option<super::types::ProxyParams> — {rendered}"
    );
    assert!(
        rendered.contains("-> Self"),
        "the builder setter must return Self — {rendered}"
    );
}

#[test]
fn computed_params_builder_build_returns_the_struct() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains("pub fn build (self) -> PhotoComputedParams"),
        "the builder build() method must return PhotoComputedParams — {rendered}"
    );
}

#[test]
fn bare_only_model_still_emits_no_builder() {
    let schema = schema(BARE_ONLY_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        !rendered.contains("ComputedParams"),
        "a model with only bare @computed fields must not emit ComputedParams — {rendered}"
    );
    assert!(
        !rendered.contains("ComputedParamsBuilder"),
        "a model with only bare @computed fields must not emit ComputedParamsBuilder — {rendered}"
    );
}

#[test]
fn computed_params_builder_has_a_setter_and_field_per_parameterized_computed_field() {
    let schema = schema(TWO_PARAMETERIZED_FIELDS_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains("struct VideoComputedParamsBuilder"),
        "VideoComputedParams must have a builder struct — {rendered}"
    );
    // Struct fields: one per parameterized computed field.
    assert!(
        rendered.contains("pub proxyUrl") || rendered.contains("pub proxyUrl :"),
        "VideoComputedParams struct must have a proxyUrl field — {rendered}"
    );
    assert!(
        rendered.contains("pub captionUrl") || rendered.contains("pub captionUrl :"),
        "VideoComputedParams struct must have a captionUrl field — {rendered}"
    );
    // Builder setters: one per parameterized computed field.
    assert!(
        rendered.contains("fn proxyUrl"),
        "VideoComputedParamsBuilder must have a proxyUrl setter — {rendered}"
    );
    assert!(
        rendered.contains("fn captionUrl"),
        "VideoComputedParamsBuilder must have a captionUrl setter — {rendered}"
    );
    // Each setter takes its own params type.
    assert!(
        rendered.contains(":: core :: option :: Option < super :: types :: ProxyParams >"),
        "the proxyUrl setter must take Option<super::types::ProxyParams> — {rendered}"
    );
    assert!(
        rendered.contains(":: core :: option :: Option < super :: types :: CaptionParams >"),
        "the captionUrl setter must take Option<super::types::CaptionParams> — {rendered}"
    );
}

#[test]
fn rpc_transport_emits_the_builder_too() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rpc, &BTreeSet::new());

    assert!(
        rendered.contains("struct PhotoComputedParamsBuilder"),
        "RPC PhotoComputedParams must have a builder struct — {rendered}"
    );
    assert!(
        rendered.contains("pub fn builder () -> PhotoComputedParamsBuilder"),
        "RPC PhotoComputedParams must have a builder() method — {rendered}"
    );
}
