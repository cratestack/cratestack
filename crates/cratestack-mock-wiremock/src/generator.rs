use cratestack_core::{Schema, TransportStyle};

use crate::config::{GeneratedWireMockFile, GeneratedWireMockPackage, WireMockGeneratorConfig};
use crate::error::WireMockGeneratorError;
use crate::mapping::build_procedure_mapping;

/// Renders one WireMock stub-mapping JSON file per `procedure` declared
/// in `schema`, under `mappings/<procedureName>.json`. See the crate
/// docs and `docs/design/wiremock-stubs.md` for scope and rationale;
/// `model` blocks (REST CRUD routes) and `transport grpc` schemas are
/// not covered — the former is silently skipped (no procedures means no
/// output, not an error), the latter is
/// [`WireMockGeneratorError::UnsupportedTransport`].
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
                    procedure: procedure.name.clone(),
                    source,
                }
            })?;
            Ok(GeneratedWireMockFile {
                file_name: format!("mappings/{}.json", procedure.name),
                contents: format!("{contents}\n"),
            })
        })
        .collect::<Result<Vec<_>, WireMockGeneratorError>>()?;

    // Deterministic output order regardless of declaration order in the
    // schema — matches `generate-dart`/`generate-typescript --check`'s
    // drift-detection expectations (stable output for an unchanged
    // schema) and makes the generated file list easy to diff in review.
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    Ok(GeneratedWireMockPackage { files })
}
