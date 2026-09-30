//! Canonical bytes of a JSON value, for checking a stored contract.

use serde_json::Value;

/// The bytes a digest is taken over: the value as compact JSON with object
/// keys sorted, which is what the canonical structs serialize to (their
/// fields are declared in lexicographic order).
pub(super) fn canonical_bytes(value: &Value) -> Vec<u8> {
    fn write(value: &Value, out: &mut Vec<u8>) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push(b'{');
                for (i, key) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    // Infallible: a `String` is always valid JSON.
                    out.extend(serde_json::to_vec(key).expect("strings serialize"));
                    out.push(b':');
                    write(&map[key], out);
                }
                out.push(b'}');
            }
            Value::Array(items) => {
                out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    write(item, out);
                }
                out.push(b']');
            }
            // `Display` of a `Value` is its compact JSON and cannot fail.
            other => out.extend(other.to_string().into_bytes()),
        }
    }
    let mut out = Vec::new();
    write(value, &mut out);
    out
}
