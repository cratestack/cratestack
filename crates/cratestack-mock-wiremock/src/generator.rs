use std::collections::BTreeSet;

use cratestack_core::{Schema, TransportStyle};

use crate::config::{GeneratedWireMockFile, GeneratedWireMockPackage, WireMockGeneratorConfig};
use crate::error::WireMockGeneratorError;
use crate::mapping::build_procedure_mapping;
use crate::model_mapping::build_model_mappings;

/// Renders one WireMock stub-mapping JSON file per `procedure` declared
/// in `schema` (`mappings/<procedureName>.json`) and five per `model`
/// (`mappings/model.<ModelName>.<list|get|create|update|delete>.json`).
/// See the crate docs and `docs/design/wiremock-stubs.md` for scope and
/// rationale; `transport grpc` schemas are rejected outright
/// ([`WireMockGeneratorError::UnsupportedTransport`]) — neither
/// procedures nor models get a stub for one.
pub fn generate_package(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
) -> Result<GeneratedWireMockPackage, WireMockGeneratorError> {
    if schema.transport == TransportStyle::Grpc {
        return Err(WireMockGeneratorError::UnsupportedTransport);
    }

    let mut files = schema
        .procedures
        .iter()
        .map(|procedure| {
            let mapping = build_procedure_mapping(schema, config, procedure)?;
            let contents = serde_json::to_string_pretty(&mapping).map_err(|source| {
                WireMockGeneratorError::Serialize {
                    subject: format!("procedure `{}`", procedure.name),
                    source,
                }
            })?;
            Ok(GeneratedWireMockFile {
                file_name: format!("mappings/{}.json", procedure.name),
                contents: format!("{contents}\n"),
            })
        })
        .collect::<Result<Vec<_>, WireMockGeneratorError>>()?;

    // Every model name, so per-field synthesis can tell a relation field
    // (its type names another declared model — populated only via
    // `include=<relation>`, excluded from the default projection this
    // generator mirrors) apart from a plain scalar/composite field.
    let model_names: BTreeSet<&str> = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect();

    for model in &schema.models {
        for (verb, mapping) in build_model_mappings(schema, config, model, &model_names)? {
            let contents = serde_json::to_string_pretty(&mapping).map_err(|source| {
                WireMockGeneratorError::Serialize {
                    subject: format!("model `{}` `{verb}` mapping", model.name),
                    source,
                }
            })?;
            files.push(GeneratedWireMockFile {
                file_name: format!("mappings/model.{}.{verb}.json", model.name),
                contents: format!("{contents}\n"),
            });
        }
    }

    // Deterministic output order regardless of declaration order in the
    // schema — matches `generate-dart`/`generate-typescript --check`'s
    // drift-detection expectations (stable output for an unchanged
    // schema) and makes the generated file list easy to diff in review.
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    Ok(GeneratedWireMockPackage { files })
}
