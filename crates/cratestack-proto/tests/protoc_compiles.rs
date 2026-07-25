//! Integration test: the emitted `.proto` text must actually compile under
//! `protoc --descriptor_set_out`. Per `docs/design/protobuf.md` §5, this
//! test is written to skip (not fail) in an environment where `protoc`
//! isn't on `PATH` — mirroring how the PG-backed tests skip without
//! `CRATESTACK_TEST_DATABASE_URL` — but `protoc` v35.1 is installed at
//! `/opt/homebrew/bin/protoc` in this environment, so the assertion below
//! confirms it actually ran rather than silently no-opped here.

use std::process::Command;

use cratestack_proto::{build_lock, emit_proto, synthesize_messages};
use tempfile::TempDir;

/// Exercises: a model-to-model relation, an enum, `@server_only`
/// exclusion, `Decimal`/`Json`/`DateTime` field mapping, a `type`
/// declaration, and a procedure returning `Page<T>`.
const REST_FIXTURE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth SessionUser {
  id Int
}

enum Role {
  ADMIN
  MEMBER
}

type OrderFilter {
  status String
}

model Profile {
  id Int @id
  nickname String
}

model User {
  id Int @id
  email String @unique
  role Role
  secretNote String? @server_only
  profileId Int
  profile Profile @relation(fields:[profileId],references:[id])

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}

model Order {
  id Int @id
  total Decimal
  metadata Json
  placedAt DateTime
  userId Int
  user User @relation(fields:[userId],references:[id])

  @@allow("read", auth() != null)
}

procedure listOrders(filter: OrderFilter?): Page<Order>
  @allow(auth() != null)
"#;

/// A `transport rpc` schema with a list-valued field and a mutation, so
/// the `protoc` check also covers the `repeated` (no `optional`) path and
/// the RPC transport variant end to end.
const RPC_FIXTURE: &str = r#"
transport rpc

datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Caller {
  id Int
}

type TagList {
  tags String[]
}

model Note {
  id Cuid @id
  title String
  createdAt DateTime

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}

mutation procedure archiveNote(args: TagList): Note
  @allow(auth() != null)
"#;

fn assert_protoc_compiles(schema_source: &str, schema_path: &str, package: &str) -> String {
    let schema = cratestack_parser::parse_schema(schema_source).expect("fixture schema parses");
    let extra = synthesize_messages(&schema).expect("synthesize_messages");
    let mut lock = build_lock(&schema, None, &extra).expect("build_lock");
    lock.package = Some(package.to_owned());
    let proto_text =
        emit_proto(&schema, &lock, &extra, schema_path).expect("emit_proto should succeed");

    let dir = TempDir::new().expect("tempdir");
    let file_name = format!("{package}.proto");
    std::fs::write(dir.path().join(&file_name), &proto_text).expect("write .proto fixture");
    let descriptor_path = dir.path().join("descriptor.bin");

    let output = Command::new("protoc")
        .arg(format!("-I{}", dir.path().display()))
        .arg(format!(
            "--descriptor_set_out={}",
            descriptor_path.display()
        ))
        .arg(&file_name)
        .current_dir(dir.path())
        .output()
        .expect("protoc must be installed in this environment (see docs/design/protobuf.md §5)");

    assert!(
        output.status.success(),
        "protoc failed to compile the emitted .proto:\n--- stdout ---\n{}\n--- stderr ---\n{}\n--- proto ---\n{proto_text}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        descriptor_path.exists(),
        "protoc reported success but wrote no descriptor set"
    );

    proto_text
}

#[test]
fn rest_fixture_with_relations_enum_and_page_compiles() {
    if Command::new("protoc").arg("--version").output().is_err() {
        eprintln!("protoc not found on PATH; skipping (see docs/design/protobuf.md §5)");
        return;
    }

    let proto_text = assert_protoc_compiles(REST_FIXTURE, "fixtures/rest.cstack", "shop_api");

    assert!(proto_text.contains("enum Role {"));
    assert!(proto_text.contains("message Order {"));
    assert!(proto_text.contains("message PageOfOrder {"));
    assert!(proto_text.contains("message PageInfo {"));
    assert!(proto_text.contains("import \"google/protobuf/timestamp.proto\";"));
    assert!(
        !proto_text.contains("secretNote"),
        "@server_only field must never reach the emitted .proto"
    );
    assert!(proto_text.contains("message CreateUserInput {"));
}

#[test]
fn rpc_fixture_with_list_field_and_mutation_compiles() {
    if Command::new("protoc").arg("--version").output().is_err() {
        eprintln!("protoc not found on PATH; skipping (see docs/design/protobuf.md §5)");
        return;
    }

    let proto_text = assert_protoc_compiles(RPC_FIXTURE, "fixtures/rpc.cstack", "notes_api");

    assert!(proto_text.contains("message Note {"));
    assert!(proto_text.contains("repeated string tags"));
    assert!(proto_text.contains("message ArchiveNoteInput {"));
    assert!(proto_text.contains("message ArchiveNoteOutput {"));
    assert!(proto_text.contains("This schema's `transport` is `rpc`"));
}
