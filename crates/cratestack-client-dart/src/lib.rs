mod builders;
mod builders_model;
mod config;
mod context;
mod dart_types;
mod generator;
mod idents;
mod naming;
mod templates;
mod templates_fragments;
mod views;
mod wire_decode;
mod wire_encode;

pub use config::{DartGeneratorConfig, DartGeneratorError, GeneratedDartFile, GeneratedDartPackage};
pub use generator::generate_package;
