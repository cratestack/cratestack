//! Shared fixture and helpers of the op-contract suites.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{digest_hex, op_contract_digests};

pub const BASE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

transport rpc

auth Principal {
  id Int
}

enum Status {
  Open
  Closed
}

model Account {
  id     Int    @id
  name   String @length(min: 1, max: 40)
  status Status
  orders Order[] @relation(fields: [id], references: [accountId])

  @@allow("read", auth() != null)
}

model Order {
  id        Int @id
  accountId Int
  total     Int
  account   Account @relation(fields: [accountId], references: [id])
}

model Note {
  id   Int    @id
  body String
}

type PingArgs {
  message String
}

type PingReply {
  echo String
}

procedure ping(args: PingArgs): PingReply
  @allow(auth() != null)
"#;

pub fn digests(source: &str) -> BTreeMap<String, String> {
    let schema = cratestack_parser::parse_schema(source).expect("schema parses");
    op_contract_digests(&schema)
        .into_iter()
        .map(|(key, digest)| (key, digest_hex(&digest)))
        .collect()
}

pub fn edit(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "base has no `{from}`");
    BASE.replacen(from, to, 1)
}

/// Keys present in both whose digest differs.
pub fn moved(source: &str) -> BTreeSet<String> {
    let (before, after) = (digests(BASE), digests(source));
    before
        .iter()
        .filter(|(key, digest)| after.get(*key).is_some_and(|d| d != *digest))
        .map(|(key, _)| key.clone())
        .collect()
}

pub fn ops_of(models: &[&str]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for model in models {
        for verb in ["list", "get", "create", "update", "delete"] {
            keys.insert(format!("model.{model}.{verb}"));
        }
    }
    keys
}

pub fn assert_unchanged(source: &str) {
    assert_eq!(digests(source), digests(BASE));
}
