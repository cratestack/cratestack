//! The `request.uri.path` of each unsigned leg, as the Dart expressions the
//! generated `unsignedPaths` returns: what `DeviceAuthBackend.unsignedPaths`
//! matches exactly when the legs go through a transport that signs.
use cratestack_core::schema::DeviceAuthRole;

use super::legs::Legs;

/// The legs a device sends with no session, in the order of the ceremony.
const UNSIGNED: [DeviceAuthRole; 5] = [
    DeviceAuthRole::SendEnrolmentCode,
    DeviceAuthRole::BeginEnrolment,
    DeviceAuthRole::ConfirmEnrolment,
    DeviceAuthRole::BeginRenewal,
    DeviceAuthRole::ConfirmRenewal,
];

/// `$prefix` is the base URI's path without its trailing slash, `$basePath`
/// the REST client's prefix; both are locals of the generated function.
pub(super) fn unsigned(legs: &Legs<'_>, is_rpc: bool) -> Vec<String> {
    UNSIGNED
        .iter()
        .filter_map(|role| legs.get(*role))
        .map(|(procedure, _)| {
            if is_rpc {
                format!(
                    "'$prefix/rpc/{}'",
                    cratestack_core::procedure_op_key(procedure, true)
                )
            } else {
                // Already escaped for a Dart literal: the route has a `$procs`.
                let route = cratestack_core::procedure_route::procedure_rest_route_path(procedure)
                    .replace('$', "\\$");
                format!("'$prefix$basePath{route}'")
            }
        })
        .collect()
}
