//! `cratestack contract digest|print`: the per-op contract digests a signed
//! client would bind (cratestack#1123). Read-only; nothing here gates CI yet.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Subcommand;
use cratestack_core::{
    Schema, client_contract_digest, digest_hex, op_contract_digests, op_contract_json,
};

use crate::cli_support::parse_schema_or_render;

#[cfg(test)]
mod tests;

#[derive(Debug, Subcommand)]
pub(crate) enum ContractAction {
    /// Print every op's contract digest and the client contract digest.
    Digest {
        #[arg(long)]
        schema: PathBuf,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Print the canonical contract JSON one op's digest is taken over:
    /// the tool for "why did this op's digest move".
    Print {
        #[arg(long)]
        schema: PathBuf,
        /// The op key: an RPC `op_id` (`procedure.ping`) or, on REST,
        /// `"<METHOD> <route template>"` (`GET /widgets/{id}`).
        #[arg(long)]
        op: String,
    },
}

pub(crate) fn run(action: ContractAction) -> Result<()> {
    match action {
        ContractAction::Digest { schema, json } => {
            print!("{}", digest_report(&parse_schema_or_render(&schema)?, json));
            Ok(())
        }
        ContractAction::Print { schema, op } => {
            println!("{}", print_report(&parse_schema_or_render(&schema)?, &op)?);
            Ok(())
        }
    }
}

fn digest_report(schema: &Schema, json: bool) -> String {
    let table = op_contract_digests(schema);
    let client = digest_hex(&client_contract_digest(schema));
    if json {
        let ops: serde_json::Map<String, serde_json::Value> = table
            .iter()
            .map(|(key, digest)| (key.clone(), digest_hex(digest).into()))
            .collect();
        let doc = serde_json::json!({ "client_contract": client, "ops": ops });
        return format!("{}\n", serde_json::to_string_pretty(&doc).expect("JSON"));
    }
    let mut out = format!("client contract  {client}\n");
    for (key, digest) in &table {
        out.push_str(&format!("{}  {key}\n", digest_hex(digest)));
    }
    out
}

fn print_report(schema: &Schema, op: &str) -> Result<String> {
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
