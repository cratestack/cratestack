//! What `device-auth-server.dart.j2` renders: every Dart expression is
//! built here, from the schema, so the templates only lay them out.
use serde::Serialize;

/// One call of a ceremony: the generated `ProceduresApi` method, its
/// argument wrapper (`procedure_wrapper_name`) and the schema type inside it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LegView {
    /// `ProceduresApi.<method>`.
    pub(crate) method: String,
    /// The generated argument wrapper class.
    pub(crate) wrapper: String,
    /// The wrapper's constructor parameter: the procedure's argument name.
    pub(crate) arg_field: String,
    /// The schema `type` the argument is.
    pub(crate) arg_type: String,
    /// `name: expr` named arguments of that type's constructor, in order.
    pub(crate) args: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) expr: String,
}

/// `Jwk(kty: jwk['kty']!, ...)` for one schema JWK `type`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct JwkHelper {
    pub(crate) name: String,
    pub(crate) type_name: String,
    /// `(identifier, wire name)` of `kty`, `crv`, `x`, `y`.
    pub(crate) fields: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SendCodeView {
    /// `ProceduresApi.<method>`.
    pub(crate) method: String,
    /// The generated argument wrapper class, which the generated method takes whole.
    pub(crate) wrapper: String,
    pub(crate) return_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BeginEnrolmentView {
    pub(crate) leg: LegView,
    /// Map elements of `EnrolmentChallenge.challenges`.
    pub(crate) challenges: Vec<String>,
    /// Map elements of `EnrolmentChallenge.data`.
    pub(crate) data: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConfirmEnrolmentView {
    pub(crate) leg: LegView,
    /// The schema `type` the answer is, the parameter of `userOf`.
    pub(crate) answer_type: String,
    /// Map elements of `DeviceEnrolment.attestations`.
    pub(crate) attestations: Vec<String>,
    /// The `proof:` field the one-time code goes in, if the schema names one.
    pub(crate) proof: Option<String>,
    pub(crate) name_expr: Option<String>,
    pub(crate) email_expr: Option<String>,
    /// Map elements of `AuthUser.claims`.
    pub(crate) claims: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BeginRenewalView {
    pub(crate) leg: LegView,
    /// Map elements of `RoleChallenge.data`.
    pub(crate) data: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DeviceAuthView {
    pub(crate) class_name: String,
    pub(crate) client_class_name: String,
    /// `lib/<library_name>.dart`, which exports every generated type.
    pub(crate) library_name: String,
    pub(crate) is_rpc: bool,
    pub(crate) adapter_class: &'static str,
    pub(crate) exception_class: &'static str,
    pub(crate) base_path_literal: String,
    pub(crate) fespalier_ref: String,
    pub(crate) send_code: Option<SendCodeView>,
    pub(crate) begin_enrolment: BeginEnrolmentView,
    pub(crate) confirm_enrolment: ConfirmEnrolmentView,
    pub(crate) begin_renewal: BeginRenewalView,
    pub(crate) confirm_renewal: LegView,
    pub(crate) revoke: LegView,
    pub(crate) jwk_helpers: Vec<JwkHelper>,
    /// Dart string literals (with `$prefix` interpolation) of the unsigned legs' paths.
    pub(crate) unsigned_paths: Vec<String>,
}
