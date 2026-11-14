//! Shared by the `fespalier_device_auth*` test binaries (cratestack#1303).
#![allow(dead_code)]

use std::collections::BTreeMap;

use cratestack_client_dart::{
    DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartPackage, generate_package,
};

pub const DEVICE_AUTH_FILE: &str = "lib/src/device_auth_server.dart";
pub const RPC: &str = include_str!("../fixtures/device_auth_rpc.cstack");
pub const REST: &str = include_str!("../fixtures/device_auth_rest.cstack");

pub fn without_markers(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("@device_auth"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn try_generate(
    source: &str,
    preset: DartPreset,
    git_ref: Option<&str>,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    let schema = cratestack_parser::parse_schema_unvalidated(source).expect("fixture parses");
    let mut config = DartGeneratorConfig {
        library_name: "device_client".to_owned(),
        preset,
        ..DartGeneratorConfig::default()
    };
    if let Some(git_ref) = git_ref {
        config.fespalier_ref = git_ref.to_owned();
    }
    generate_package(&schema, &config)
}

pub fn generate_from(
    source: &str,
    preset: DartPreset,
    git_ref: Option<&str>,
) -> GeneratedDartPackage {
    try_generate(source, preset, git_ref).unwrap_or_else(|error| panic!("generates: {error}"))
}

pub fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
        .contents
        .as_str()
}

pub fn contents(package: &GeneratedDartPackage) -> BTreeMap<&str, &str> {
    package
        .files
        .iter()
        .map(|file| (file.file_name.as_str(), file.contents.as_str()))
        .collect()
}
