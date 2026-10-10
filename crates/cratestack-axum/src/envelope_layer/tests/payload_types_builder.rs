//! `build()` refuses a payload-type configuration that could not be served
//! (cratestack#1168).

use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};

fn base() -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rpc("")
}

fn refused(builder: EnvelopeLayerBuilder, why: &str) {
    let error = builder.build().expect_err(why);
    assert!(
        error.to_string().to_lowercase().contains("payload"),
        "{why}: {error}"
    );
}

#[test]
fn the_default_is_cbor_and_a_listed_set_builds() {
    base().build().expect("the default builds");
    base()
        .payload_media_types(
            ["application/cbor", "application/x-www-form-urlencoded"],
            ["application/json"],
        )
        .build()
        .expect("a form in, JSON out");
}

#[test]
fn envelopes_streams_and_multiparts_are_never_sealable() {
    for bad in [
        "application/cose",
        "application/cose-key",
        "application/cbor-seq",
        "text/event-stream",
        "multipart/form-data",
    ] {
        refused(
            base().payload_media_types(["application/cbor", bad], ["application/cbor"]),
            bad,
        );
        refused(
            base().payload_media_types(["application/cbor"], ["application/cbor", bad]),
            bad,
        );
    }
}

#[test]
fn a_type_outside_the_grammar_is_refused() {
    for bad in [
        "application/json; charset=utf-8",
        "Application/JSON",
        "application/*",
        "*/*",
        "json",
        "",
    ] {
        refused(base().payload_media_types([bad], ["application/cbor"]), bad);
        refused(base().payload_media_types(["application/cbor"], [bad]), bad);
    }
}

#[test]
fn an_empty_request_set_is_refused() {
    refused(
        base().payload_media_types(Vec::<String>::new(), ["application/cbor"]),
        "no request type",
    );
}

#[test]
fn a_response_set_with_neither_cbor_nor_json_is_refused() {
    // A layer error is sealed in a negotiated type the transport's error
    // codec can write: CBOR or JSON.
    refused(
        base().payload_media_types(["application/cbor"], ["application/x-www-form-urlencoded"]),
        "form only",
    );
    refused(
        base().payload_media_types(["application/cbor"], Vec::<String>::new()),
        "empty",
    );
}
