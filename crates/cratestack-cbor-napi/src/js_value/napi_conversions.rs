//! `FromNapiValue`/`ToNapiValue` for [`JsCborValue`]. Split out of the
//! parent module so everything that names a napi FFI type sits in one
//! place, under one `cfg` — the crate root explains why that separation
//! is what lets `cargo test -p cratestack-cbor-napi` link at all (the
//! `napi_*` C symbols only exist inside a running Node process, so any
//! *reachable* path constructing them fails at link time; nothing here is
//! reachable from a test, so the linker drops it).
//!
//! The recursion is entirely napi's: `Vec<JsCborValue>` and
//! `BTreeMap<String, JsCborValue>` already have blanket conversions, and
//! they call back into the impls below for each element — which is what
//! makes a `Uint8Array` nested anywhere in the tree get the same
//! treatment as one at the top level.

use std::collections::BTreeMap;
use std::ptr;

use cratestack_core::Value;
use napi::bindgen_prelude::{FromNapiValue, Null, ToNapiValue, TypedArrayType, Uint8Array};
use napi::{Result, ValueType, check_status, sys, type_of};

use super::JsCborValue;

impl FromNapiValue for JsCborValue {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        // Everything that isn't object-typed — booleans, numbers,
        // strings, `null`, `BigInt` — is delegated to napi's own
        // `serde_json::Value` conversion rather than re-derived here, so
        // the scalar behaviour (including its `BigInt` handling and its
        // rejection of functions/`undefined`/symbols/externals, with the
        // exact same error messages) is unchanged by cratestack#783.
        if type_of!(env, napi_val)? != ValueType::Object {
            let json = unsafe { serde_json::Value::from_napi_value(env, napi_val)? };
            return Ok(JsCborValue(Value::from_plain_json(json)));
        }

        if let Some(bytes) = unsafe { as_bytes(env, napi_val) }? {
            return Ok(JsCborValue(Value::Bytes(bytes)));
        }

        let mut is_array = false;
        check_status!(
            unsafe { sys::napi_is_array(env, napi_val, &mut is_array) },
            "Failed to detect whether the given JS value is an array"
        )?;
        if is_array {
            let items = unsafe { Vec::<JsCborValue>::from_napi_value(env, napi_val)? };
            return Ok(JsCborValue(Value::List(
                items.into_iter().map(|item| item.0).collect(),
            )));
        }

        let entries = unsafe { BTreeMap::<String, JsCborValue>::from_napi_value(env, napi_val)? };
        Ok(JsCborValue(Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key, value.0))
                .collect(),
        )))
    }
}

impl ToNapiValue for JsCborValue {
    unsafe fn to_napi_value(env: sys::napi_env, value: Self) -> Result<sys::napi_value> {
        match value.0 {
            Value::Null => unsafe { Null::to_napi_value(env, Null) },
            Value::Bool(value) => unsafe { bool::to_napi_value(env, value) },
            // Through `serde_json::Number`, not `i64` directly: napi's
            // `Number` conversion is what decides between a JS number and
            // a `BigInt` at the ±(2^53 - 1) boundary, and that split is
            // pre-existing behaviour worth keeping byte-for-byte.
            Value::Int(value) => unsafe {
                serde_json::Value::to_napi_value(env, serde_json::Value::Number(value.into()))
            },
            // Deliberately *not* through `serde_json::Number`, which
            // cannot represent `NaN`/`±Infinity` — see the parent module
            // docs.
            Value::Float(value) => unsafe { f64::to_napi_value(env, value) },
            Value::String(value) => unsafe { String::to_napi_value(env, value) },
            Value::Bytes(bytes) => unsafe {
                Uint8Array::to_napi_value(env, Uint8Array::new(bytes))
            },
            Value::List(items) => unsafe {
                Vec::<JsCborValue>::to_napi_value(env, items.into_iter().map(JsCborValue).collect())
            },
            Value::Map(entries) => unsafe {
                BTreeMap::<String, JsCborValue>::to_napi_value(
                    env,
                    entries
                        .into_iter()
                        .map(|(key, value)| (key, JsCborValue(value)))
                        .collect(),
                )
            },
        }
    }
}

/// `Some(bytes)` for the JS shapes that carry an unambiguous byte
/// sequence, `None` for every other object (arrays, plain objects, and
/// non-`Uint8` typed arrays, which fall through to their previous
/// handling). Called only after the caller has established the value is
/// object-typed.
unsafe fn as_bytes(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Option<Vec<u8>>> {
    let mut is_typedarray = false;
    check_status!(
        unsafe { sys::napi_is_typedarray(env, napi_val, &mut is_typedarray) },
        "Failed to detect whether the given JS value is a TypedArray"
    )?;
    if is_typedarray {
        let mut typed_array_type = 0;
        let mut length = 0;
        let mut data = ptr::null_mut();
        let mut arraybuffer = ptr::null_mut();
        let mut byte_offset = 0;
        check_status!(
            unsafe {
                sys::napi_get_typedarray_info(
                    env,
                    napi_val,
                    &mut typed_array_type,
                    &mut length,
                    &mut data,
                    &mut arraybuffer,
                    &mut byte_offset,
                )
            },
            "Failed to read TypedArray info"
        )?;
        // `data` is already adjusted by `byte_offset` (per the Node-API
        // contract), and for a `Uint8` element type `length` is the
        // element count, which equals the byte count — so a subarray view
        // copies its own window, not the whole backing buffer.
        //
        // `Uint8` only — deliberately *not* `Uint8Clamped`, even though
        // its element type is also a byte. `serde-wasm-bindgen` (the
        // `@cratestack/cbor-web` side) recognises `Uint8Array` and
        // `ArrayBuffer` and nothing else, and a `Uint8ClampedArray` is not
        // a `Uint8Array` subclass, so accepting it here would make the
        // same TypeScript client put a different payload on the wire
        // depending on which runtime it happened to load in. Matching the
        // narrower set keeps the two builds interchangeable, which is the
        // whole point of shipping both.
        return Ok(match TypedArrayType::from(typed_array_type) {
            TypedArrayType::Uint8 => Some(unsafe { copy_bytes(data, length) }),
            _ => None,
        });
    }

    let mut is_arraybuffer = false;
    check_status!(
        unsafe { sys::napi_is_arraybuffer(env, napi_val, &mut is_arraybuffer) },
        "Failed to detect whether the given JS value is an ArrayBuffer"
    )?;
    if is_arraybuffer {
        let mut data = ptr::null_mut();
        let mut length = 0;
        check_status!(
            unsafe { sys::napi_get_arraybuffer_info(env, napi_val, &mut data, &mut length) },
            "Failed to read ArrayBuffer info"
        )?;
        return Ok(Some(unsafe { copy_bytes(data, length) }));
    }

    Ok(None)
}

/// Copies `length` bytes out of a JS-owned buffer into an owned `Vec`.
/// Copying rather than borrowing is required: the returned `Value` long
/// outlives this call, and the JS buffer can be detached or moved by GC
/// at any point after it.
///
/// # Safety
///
/// `data` must either be null or point to at least `length` readable
/// bytes — which is exactly what `napi_get_typedarray_info` /
/// `napi_get_arraybuffer_info` guarantee for the value they described.
unsafe fn copy_bytes(data: *mut std::ffi::c_void, length: usize) -> Vec<u8> {
    // A zero-length typed array is allowed to report a null data pointer,
    // which `from_raw_parts` would treat as UB even at length 0.
    if data.is_null() || length == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec()
}
