//! The error every `ClientEnvelope` call throws or rejects with: a plain
//! JS object `{ code, message }`.

use cratestack_core::CratestackError;
use js_sys::{Object, Reflect};
use wasm_bindgen::JsValue;

/// `code` of a failed verification: no detail, by design (ADR 0006 §10), so
/// the web client is no more of an oracle than the server.
pub(super) const REJECTED: &str = "rejected";
/// `code` of local misuse: a binding of the wrong shape, a bad key. It
/// depends on local state only, never on the received bytes.
pub(super) const MISUSE: &str = "misuse";

/// The one mapping from the Rust errors, so no call site decides what a
/// caller sees.
pub(super) fn from_error(error: CratestackError) -> JsValue {
    match error {
        CratestackError::Unauthorized(_) => object(REJECTED, ""),
        CratestackError::Internal(message) | CratestackError::Validation(message) => {
            object(MISUSE, &message)
        }
        other => object(MISUSE, &other.to_string()),
    }
}

/// Misuse that this layer found before `cratestack-cose` could.
pub(super) fn misuse(message: &str) -> JsValue {
    object(MISUSE, message)
}

fn object(code: &str, message: &str) -> JsValue {
    let error = Object::new();
    // Setting a property on a fresh plain object cannot fail.
    let _ = Reflect::set(&error, &"code".into(), &code.into());
    let _ = Reflect::set(&error, &"message".into(), &message.into());
    error.into()
}
