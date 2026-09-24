//! Built-in scalar → JSON Schema. Each row follows what `serde_json`
//! actually does with the Rust type codegen emits for that scalar
//! (`crate::procedure::type_tokens`, `crate::shared::types`), measured,
//! not assumed. The round-trip suites (`cratestack-api`'s and
//! `cratestack-pg`'s `tests/json_schema_*.rs`) serialize real generated
//! types against these schemas, so a row that drifts from serde fails
//! there, not in an agent's hands.
//!
//! Where serde accepts more on input than it ever emits (`Uuid`'s simple
//! and braced forms, chrono's space separator, `BigDecimal` from a JSON
//! number), the schema describes the emitted form. An agent that follows
//! the schema then sends something serde accepts. Serde being more
//! lenient than the schema is safe; the reverse is the bug this module
//! exists to prevent. The few places where JSON Schema cannot be as
//! strict as serde are listed in `json_schema.rs`'s module doc.
//!
//! String formats carry an anchored `pattern` next to `format`. In JSON
//! Schema 2020-12, `format` is an annotation unless the validator opts in,
//! so `format` alone would let `"yesterday"` through as a `DateTime`.

use serde_json::{Value, json};

use crate::shared::decimal_backend::DecimalBackend;

/// `chrono::DateTime<Utc>` serializes as RFC 3339 with a `Z` offset and
/// as many fractional digits as it needs (`2023-11-14T22:13:20.123Z`).
/// Its deserializer takes any offset, and lowercase `t`/`z`, which RFC
/// 3339 allows too. A missing offset is rejected, so the pattern requires
/// one.
const DATE_TIME_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$";

/// `uuid::Uuid` serializes hyphenated and lowercase. It also parses the
/// simple, braced and URN forms, and uppercase hex. The schema allows
/// hyphenated in either case.
const UUID_PATTERN: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";

/// `rust_decimal` with the workspace's `serde-str` feature: always a JSON
/// string, never in exponent form (`"0.0000001"`, `"-0.0000000005"`). Its
/// deserializer rejects JSON numbers outright, which is why `Decimal` is
/// not `"type": "number"` for this backend.
const RUST_DECIMAL_PATTERN: &str = "^-?[0-9]+(\\.[0-9]+)?$";

/// `bigdecimal` with `serde`: also a JSON string, but in exponent form
/// once the exponent is large enough (`"1E-7"`, `"1e+30"`, `"-5E-10"`),
/// so this backend's pattern has to accept an exponent. It also accepts a
/// JSON number on input; the schema still says string, so an agent never
/// sends a float that has already lost precision.
const BIG_DECIMAL_PATTERN: &str = "^-?[0-9]+(\\.[0-9]+)?([eE][+-]?[0-9]+)?$";

/// What a built-in name maps to. `None` from [`builtin_scalar`] means the
/// name isn't a scalar at all (a declaration, or a generic like `Page`).
pub(super) enum Scalar {
    Mapped(Value),
    NoFaithfulMapping(&'static str),
    NeedsDecimalBackend,
}

pub(super) fn builtin_scalar(name: &str, decimal: Option<DecimalBackend>) -> Option<Scalar> {
    Some(match name {
        // `Cuid` generates as a plain `String`; nothing checks the shape.
        "String" | "Cuid" => Scalar::Mapped(json!({ "type": "string" })),
        // `i64`. serde_json rejects a float-shaped `1.0` that JSON
        // Schema's `integer` accepts; see the module doc in
        // `json_schema.rs`.
        "Int" => Scalar::Mapped(int()),
        // `f64`. Integers deserialize into it too.
        "Float" => Scalar::Mapped(json!({ "type": "number" })),
        "Boolean" => Scalar::Mapped(json!({ "type": "boolean" })),
        "DateTime" => Scalar::Mapped(json!({
            "type": "string",
            "format": "date-time",
            "pattern": DATE_TIME_PATTERN,
        })),
        "Uuid" => Scalar::Mapped(json!({
            "type": "string",
            "format": "uuid",
            "pattern": UUID_PATTERN,
        })),
        // `Vec<u8>`: serde_json writes an array of integers. The lenient
        // deserializer (`cratestack_core::lenient_bytes`, cratestack#783)
        // adds CBOR byte strings, which JSON can't express, and still
        // rejects a base64 string.
        "Bytes" => Scalar::Mapped(byte_array()),
        "Decimal" => match decimal {
            Some(DecimalBackend::RustDecimal) => {
                Scalar::Mapped(decimal_string(RUST_DECIMAL_PATTERN))
            }
            Some(DecimalBackend::BigDecimal) => Scalar::Mapped(decimal_string(BIG_DECIMAL_PATTERN)),
            None => Scalar::NeedsDecimalBackend,
        },
        "Json" => Scalar::NoFaithfulMapping(
            "it deserializes from every JSON value (`cratestack_core::Value`'s untagged \
             visitor), so the only schema that matches serde is the permissive `{}` — which \
             tells an agent nothing, and which this generator never emits",
        ),
        "Vector" => Scalar::NoFaithfulMapping(
            "`Vector(n)` is a `Vec<f32>` whose dimension serde never checks, and it can only \
             be declared behind the `pgvector` feature, which no round-trip suite builds, so \
             no mapping has been verified against its serde output",
        ),
        "Geography" | "Geometry" => Scalar::NoFaithfulMapping(
            "spatial values are EWKB bytes behind the `postgis` feature, which no round-trip \
             suite builds, so no mapping has been verified against their serde output",
        ),
        _ => return None,
    })
}

/// `i64`, also used by `Page`'s and `PageInput`'s counters.
pub(super) fn int() -> Value {
    json!({ "type": "integer", "minimum": i64::MIN, "maximum": i64::MAX })
}

fn byte_array() -> Value {
    json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 0, "maximum": 255 },
    })
}

fn decimal_string(pattern: &str) -> Value {
    json!({ "type": "string", "pattern": pattern })
}
