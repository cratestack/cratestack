//! The idempotency fingerprint of a `tools/call`: what "the same request"
//! means for a key reused over MCP.
//!
//! L3 takes the fingerprint already computed, because what makes two
//! requests "the same" is a transport fact (`cratestack-exec`'s `OpInput`
//! docs). On HTTP it is method, path + query, content type and body. The
//! MCP counterpart is the tool name and the arguments, and nothing else:
//! `_meta` carries protocol bookkeeping (the protocol version, client info,
//! the key itself) that a retry may legitimately change.
//!
//! The arguments are hashed in a canonical form, object keys sorted at every
//! depth, so a client that reorders keys on retry replays instead of
//! drawing `idempotency_key_conflict`. That does not depend on whether
//! some crate in the graph turned on `serde_json`'s `preserve_order`.
//!
//! A domain prefix keeps an MCP digest from ever equalling an HTTP one;
//! the principal namespace (`mcp:` prefix, `src/idempotency.rs`) keeps them
//! apart anyway, and this is the second wall.

use serde_json::Value;
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"cratestack-mcp/tools/call\0";

pub(crate) fn fingerprint(tool: &str, arguments: &Value) -> [u8; 32] {
    let mut canonical = String::new();
    write_canonical(arguments, &mut canonical);
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(tool.as_bytes());
    hasher.update(b"\0");
    hasher.update(canonical.as_bytes());
    hasher.finalize().into()
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&object[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::fingerprint;

    #[test]
    fn key_order_does_not_change_the_fingerprint() {
        let a = json!({ "amount": 5, "to": { "id": 1, "bank": "x" } });
        let b = json!({ "to": { "bank": "x", "id": 1 }, "amount": 5 });
        assert_eq!(fingerprint("transfer", &a), fingerprint("transfer", &b));
    }

    #[test]
    fn tool_and_values_do() {
        let a = json!({ "amount": 5 });
        assert_ne!(
            fingerprint("transfer", &a),
            fingerprint("refund", &a),
            "the same arguments to another tool are another request"
        );
        assert_ne!(
            fingerprint("transfer", &a),
            fingerprint("transfer", &json!({ "amount": 6 }))
        );
        assert_ne!(
            fingerprint("transfer", &json!({ "a": "1" })),
            fingerprint("transfer", &json!({ "a": 1 })),
            "a string and a number are different arguments"
        );
    }
}
