//! A `DeviceAuthServer` over the generated client (cratestack#1303):
//! `--preset fespalier`, for a schema whose procedures carry the
//! device-auth marker, emits `lib/src/device_auth_server.dart`, the class
//! fespalier's `DeviceAuthBackend` calls for each leg of a device's
//! enrolment, renewal and revocation.
//!
//! The parser has already checked every shape this reads
//! (`cratestack-parser`'s `validate::device_auth`), and the views here
//! follow its table field for field. A schema that did not pass it is
//! refused with [`DartGeneratorError::DeviceAuthSchema`], not a panic.
mod enrolment;
mod legs;
mod paths;
mod renewal;
mod view;

use cratestack_core::schema::procedure_device_auth;
use cratestack_core::{Schema, TransportStyle};

pub(crate) use view::DeviceAuthView;

use crate::config::{DartGeneratorConfig, DartGeneratorError, DartPreset};
use crate::idents::{escape_dart_string, to_pascal_case};

pub(crate) const OUTPUT_PATH: &str = "lib/src/device_auth_server.dart";
pub(crate) const EXPORT: &str = "export 'src/device_auth_server.dart';";

/// Whether the fespalier preset emits the class for `schema`: it is the only
/// preset that does, and only for a schema with at least one marked procedure.
pub(crate) fn applies(schema: &Schema, config: &DartGeneratorConfig) -> bool {
    config.preset == DartPreset::Fespalier
        && schema
            .procedures
            .iter()
            .any(|procedure| procedure_device_auth(procedure).is_some())
}

pub(crate) fn build(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<Option<DeviceAuthView>, DartGeneratorError> {
    if config.preset != DartPreset::Fespalier {
        return Ok(None);
    }
    let Some(found) = legs::Legs::of(schema)? else {
        return Ok(None);
    };
    let is_rpc = schema.transport == TransportStyle::Rpc;
    let mut jwk_helpers = Vec::new();
    let begin_enrolment = enrolment::begin(&found, &mut jwk_helpers)?;
    let prefix = to_pascal_case(&config.library_name);
    Ok(Some(DeviceAuthView {
        class_name: format!("{prefix}DeviceAuthServer"),
        client_class_name: format!("{prefix}CratestackClient"),
        library_name: config.library_name.clone(),
        is_rpc,
        adapter_class: if is_rpc {
            "CratestackRpcHttpAdapter"
        } else {
            "CratestackHttpAdapter"
        },
        exception_class: if is_rpc {
            "CratestackRpcException"
        } else {
            "CratestackRestException"
        },
        base_path_literal: escape_dart_string(&config.base_path),
        fespalier_ref: config.fespalier_ref.clone(),
        send_code: enrolment::send_code(&found)?,
        begin_enrolment,
        confirm_enrolment: enrolment::confirm(&found)?,
        begin_renewal: renewal::begin(&found)?,
        confirm_renewal: renewal::confirm(&found)?,
        revoke: renewal::revoke(&found)?,
        jwk_helpers,
        unsigned_paths: paths::unsigned(&found, is_rpc),
    }))
}
