//! Synthesizes a deterministic example JSON value for a procedure's
//! return type, recursively, against the schema's own models/types/enums.
//!
//! Design choice (see `docs/design/wiremock-stubs.md` "What does a stub
//! return?"): fixed per-scalar-type defaults, not random values or a
//! seed. Two consecutive `generate_package` runs against an unchanged
//! schema always produce byte-identical output — the same property
//! `generate-dart`/`generate-typescript --check` rely on for
//! drift-detection, and the property that makes it safe to gitignore
//! generated stubs and regenerate them in CI rather than committing them
//! (see the design doc's "Where do generated stubs live" section).
//! Schema-declared examples (a hypothetical `@example(...)` attribute)
//! would be strictly better and are left as a documented follow-up.

use cratestack_core::{Field, Schema, TypeArity, TypeRef};
use serde_json::{Map, Value, json};

use crate::error::WireMockGeneratorError;

/// Builds an example JSON value for `type_ref`, the way the real server's
/// JSON codec would encode an instance of it on the wire (see
/// `crates/cratestack-client-dart/src/wire_encode.rs` for the sibling
/// scalar table this mirrors: `DateTime` -> ISO-8601 string, `Bytes` ->
/// array of byte values, everything else passed through as-is).
///
/// `in_progress` tracks the model/type names currently being expanded
/// along the current recursion path, so a self-referential schema (`type
/// Comment { replies: Comment[] }`) terminates instead of overflowing
/// the stack — see the module-level cycle-breaking rule documented on
/// [`WireMockGeneratorError::UnbreakableCycle`].
pub(crate) fn synthesize(
    schema: &Schema,
    procedure_name: &str,
    type_ref: &TypeRef,
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    if let Some(item) = type_ref.page_item() {
        let inner = synthesize_named(schema, procedure_name, item, in_progress)?;
        return Ok(json!({
            "items": [inner],
            "totalCount": 1,
            "pageInfo": {
                "limit": Value::Null,
                "offset": Value::Null,
                "hasNextPage": false,
                "hasPreviousPage": false,
            },
        }));
    }

    if type_ref.is_find_many() {
        let item_name = type_ref
            .find_many_item()
            .map(|item| item.name.as_str())
            .unwrap_or("?");
        return Err(WireMockGeneratorError::UnsupportedReturnType {
            procedure: procedure_name.to_owned(),
            type_name: format!("FindMany<{item_name}>"),
            reason: "FindMany<T>'s cursor/edge wire shape isn't modeled by this generator yet",
        });
    }

    let is_composite = is_model_or_type(schema, &type_ref.name);
    if is_composite && in_progress.contains(&type_ref.name) {
        // A required field can't terminate the cycle: there is no finite
        // JSON value for it. An optional or repeated field can — reuse
        // "absent"/"empty" as the natural base case, same as a
        // hand-written fixture would.
        return match type_ref.arity {
            TypeArity::Required => Err(WireMockGeneratorError::UnbreakableCycle {
                procedure: procedure_name.to_owned(),
                type_name: type_ref.name.clone(),
            }),
            TypeArity::Optional => Ok(Value::Null),
            TypeArity::List => Ok(Value::Array(Vec::new())),
        };
    }

    // `Optional`/`List` don't just wrap a successfully-synthesized `base`
    // value — they're also the escape hatch for a cycle that closes
    // *underneath* this field rather than back on this exact type name.
    // E.g. `type A { b: B[] } type B { a: A }` (both fields required):
    // when synthesizing `A`, the cycle guard above never fires for `b`
    // (its own name, `B`, isn't in `in_progress` yet — only `A` is), so
    // synthesizing `B`'s contents is attempted, which is where `a: A`
    // hits the real cycle and fails with `UnbreakableCycle`. That error
    // is about `A`, not about `b: B[]`'s own arity — but `b` being a
    // `List` is exactly the kind of step the module doc and
    // `WireMockGeneratorError::UnbreakableCycle`'s own message promise
    // can terminate a cycle, so a `List`/`Optional` field catches an
    // `UnbreakableCycle` bubbling up from computing its own base value
    // and substitutes the natural "zero or none" instance instead of
    // propagating it — matching what the direct-repeat branch above
    // already does, just for a cycle that closes one or more levels
    // deeper instead of on this exact `TypeRef`.
    match type_ref.arity {
        TypeArity::Required => synthesize_base(schema, procedure_name, type_ref, in_progress),
        TypeArity::Optional => {
            match synthesize_base(schema, procedure_name, type_ref, in_progress) {
                Ok(base) => Ok(base),
                Err(WireMockGeneratorError::UnbreakableCycle { .. }) => Ok(Value::Null),
                Err(other) => Err(other),
            }
        }
        TypeArity::List => match synthesize_base(schema, procedure_name, type_ref, in_progress) {
            Ok(base) => Ok(Value::Array(vec![base])),
            Err(WireMockGeneratorError::UnbreakableCycle { .. }) => Ok(Value::Array(Vec::new())),
            Err(other) => Err(other),
        },
    }
}

/// Like [`synthesize`], but for a nested `TypeRef` that isn't itself
/// arity-wrapped in a way the caller wants applied again (currently only
/// `Page<T>`'s item type) — separated out so `Page<T>`'s single synthesized
/// item still participates in the same cycle guard as everything else.
fn synthesize_named(
    schema: &Schema,
    procedure_name: &str,
    type_ref: &TypeRef,
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    synthesize(schema, procedure_name, type_ref, in_progress)
}

/// The value for `type_ref.name` (plus `int_args` for `Vector(n)`)
/// ignoring `type_ref.arity` — arity wrapping (`List` -> one-element
/// array, `Optional` -> present, `Required` -> present) happens in
/// [`synthesize`], the caller.
fn synthesize_base(
    schema: &Schema,
    procedure_name: &str,
    type_ref: &TypeRef,
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    if let Some(dim) = type_ref.vector_dim() {
        return Ok(Value::Array(vec![json!(0.0); dim as usize]));
    }

    match type_ref.name.as_str() {
        "String" => Ok(json!("string")),
        // Distinct-looking placeholders for the two ID-flavored string
        // scalars, so a stub's `id`-shaped fields don't collide with a
        // plain `String` field's value in a naive test assertion.
        "Cuid" => Ok(json!("clxxxxxxxxxxxxxxxxxxxxxxxx")),
        "Uuid" => Ok(json!("00000000-0000-0000-0000-000000000000")),
        "Int" => Ok(json!(0)),
        "Float" => Ok(json!(0.0)),
        "Boolean" => Ok(json!(true)),
        // Matches `wire_encode.rs`'s `DateTime` -> `toIso8601String()`
        // shape (a valid RFC 3339 UTC instant); not asserted
        // byte-for-byte against the Rust server's own `chrono` serde
        // format today — see docs/design/wiremock-stubs.md's open
        // questions.
        "DateTime" => Ok(json!("1970-01-01T00:00:00Z")),
        // No schema-declared shape to synthesize against; an empty
        // object is the simplest valid instance.
        "Json" => Ok(json!({})),
        // Matches `wire_encode.rs`'s `Bytes` -> `.toList()` shape (a JSON
        // array of byte values, not a base64 string); empty is the
        // simplest valid instance.
        "Bytes" => Ok(Value::Array(Vec::new())),
        other => synthesize_named_reference(schema, procedure_name, other, in_progress),
    }
}

fn synthesize_named_reference(
    schema: &Schema,
    procedure_name: &str,
    name: &str,
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    if let Some(model) = schema.models.iter().find(|model| model.name == name) {
        return synthesize_object(schema, procedure_name, name, &model.fields, in_progress);
    }
    if let Some(type_decl) = schema.types.iter().find(|type_decl| type_decl.name == name) {
        return synthesize_object(schema, procedure_name, name, &type_decl.fields, in_progress);
    }
    if let Some(enum_decl) = schema.enums.iter().find(|enum_decl| enum_decl.name == name) {
        return enum_decl
            .variants
            .first()
            .map(|variant| json!(variant.name))
            .ok_or_else(|| WireMockGeneratorError::UnsupportedReturnType {
                procedure: procedure_name.to_owned(),
                type_name: name.to_owned(),
                reason: "enum declares no variants to pick an example from",
            });
    }

    Err(WireMockGeneratorError::UnknownType {
        procedure: procedure_name.to_owned(),
        type_name: name.to_owned(),
    })
}

fn synthesize_object(
    schema: &Schema,
    procedure_name: &str,
    type_name: &str,
    fields: &[Field],
    in_progress: &mut Vec<String>,
) -> Result<Value, WireMockGeneratorError> {
    in_progress.push(type_name.to_owned());
    let mut object = Map::with_capacity(fields.len());
    for field in fields {
        let value = synthesize(schema, procedure_name, &field.ty, in_progress);
        // Pop before propagating an error too, so a caller that catches
        // and continues (none does today, but this keeps the invariant
        // "in_progress reflects only the still-active call stack" true
        // regardless of control flow) doesn't see a stale entry.
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                in_progress.pop();
                return Err(error);
            }
        };
        object.insert(field.name.clone(), value);
    }
    in_progress.pop();
    Ok(Value::Object(object))
}

fn is_model_or_type(schema: &Schema, name: &str) -> bool {
    schema.models.iter().any(|model| model.name == name)
        || schema.types.iter().any(|type_decl| type_decl.name == name)
}
