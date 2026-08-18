mod builders;
mod builders_model;
mod config;
mod context;
mod dart_types;
mod find_many_views;
mod generator;
mod grpc;
mod idents;
mod naming;
mod riverpod;
mod templates;
mod templates_fragments;
mod views;
mod wire_decode;
mod wire_encode;

pub use config::{
    DEFAULT_NATIVE_CBOR, DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartFile,
    GeneratedDartPackage,
};
pub use generator::generate_package;
