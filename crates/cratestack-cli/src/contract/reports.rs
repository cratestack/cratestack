//! The read-only reports of `cratestack contract digest|print`.

use anyhow::{Result, bail};
use cratestack_core::{
    Schema, client_contract_digest, digest_hex, op_contract_digests, op_contract_json,
};

pub(super) fn digest_report(schema: &Schema, json: bool) -> Result<String> {
    let table = op_contract_digests(schema);
    let client = digest_hex(&client_contract_digest(schema));
    if json {
        let ops: serde_json::Map<String, serde_json::Value> = table
            .into_iter()
            .map(|(key, digest)| (key, digest_hex(&digest).into()))
            .collect();
        let doc = serde_json::json!({ "client_contract": client, "ops": ops });
        return Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?));
    }
    let mut out = format!("client contract  {client}\n");
    for (key, digest) in &table {
        out.push_str(&format!("{}  {key}\n", digest_hex(digest)));
    }
    Ok(out)
}

pub(super) fn print_report(schema: &Schema, op: &str) -> Result<String> {
    match op_contract_json(schema, op) {
        Some(json) => Ok(json),
        None => {
            let known: Vec<String> = op_contract_digests(schema)
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            bail!("no op `{op}` in this schema; ops: {}", known.join(", "))
        }
    }
}
