//! cratestack#743 review finding — `generate-wiremock` used to advertise
//! every `@@internal(...)`-suppressed model action as a working stub
//! (e.g. a stateful `201 Created` for a `create` the real server
//! suppresses with a `405`/`404`), handing a mock consumer a contract
//! the real server does not honor. `model_mapping::build_model_mappings`
//! (both the stateful REST path and the static RPC path) now consults
//! `cratestack_core::model_internal_actions` — the same single source of
//! truth every other generation surface consults
//! (`docs/design/route-suppression.md`) — before emitting a mapping.
//!
//! Covers both transports, since REST (stateful,
//! `model_mapping::build_model_mappings` -> `model_state::
//! build_stateful_rest_mappings`) and RPC (static, ->
//! `model_mapping::build_static_rpc_mappings`) build mappings through
//! entirely separate code paths that both had to be fixed.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

const PG_DATASOURCE: &str = "datasource db {
  provider = \"postgresql\"
  url = env(\"DATABASE_URL\")
}
";

fn file_names(package: &cratestack_mock_wiremock::GeneratedWireMockPackage) -> Vec<&str> {
    package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect()
}

#[test]
fn rest_suppressed_create_gets_no_mapping_file_but_the_rest_survive() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String

  @@allow(\"read\", auth() != null)
  @@allow(\"create\", auth() != null)
  @@allow(\"update\", auth() != null)
  @@allow(\"delete\", auth() != null)
  @@internal(\"create\")
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names = file_names(&package);

    assert!(
        !names.contains(&"mappings/model.Widget.create.json"),
        "a suppressed create must not get a stub mapping — a consumer developing against this \
         mock would code against a contract the real server (405 on POST /widgets) does not \
         honor. got: {names:?}"
    );
    for verb in ["list", "get", "update", "delete"] {
        let expected = format!("mappings/model.Widget.{verb}.json");
        assert!(
            names.contains(&expected.as_str()),
            "unsuppressed verb `{verb}` must still get a stub mapping, got: {names:?}"
        );
    }
}

#[test]
fn rest_fully_suppressed_model_produces_no_mappings_at_all() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Gadget {{
  id Int @id
  name String

  @@allow(\"read\", auth() != null)
  @@allow(\"create\", auth() != null)
  @@allow(\"update\", auth() != null)
  @@allow(\"delete\", auth() != null)
  @@internal(\"all\")
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names = file_names(&package);

    assert!(
        names.iter().all(|name| !name.contains("Gadget")),
        "an @@internal(\"all\") model must produce zero stub mappings, got: {names:?}"
    );
}

#[test]
fn rpc_suppressed_create_gets_no_mapping_file_but_the_rest_survive() {
    let schema = schema(&format!(
        "transport rpc

{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String

  @@allow(\"read\", auth() != null)
  @@allow(\"create\", auth() != null)
  @@allow(\"update\", auth() != null)
  @@allow(\"delete\", auth() != null)
  @@internal(\"create\")
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names = file_names(&package);

    assert!(
        !names.contains(&"mappings/model.Widget.create.json"),
        "a suppressed create must not get a stub mapping under transport rpc either — a POST \
         to /rpc/model.Widget.create should be the pre-existing unknown-op-id NotFound, not a \
         stub advertising 201. got: {names:?}"
    );
    for verb in ["list", "get", "update", "delete"] {
        let expected = format!("mappings/model.Widget.{verb}.json");
        assert!(
            names.contains(&expected.as_str()),
            "unsuppressed verb `{verb}` must still get a stub mapping under transport rpc, \
             got: {names:?}"
        );
    }
}

#[test]
fn rpc_fully_suppressed_model_produces_no_mappings_at_all() {
    let schema = schema(&format!(
        "transport rpc

{PG_DATASOURCE}
model Gadget {{
  id Int @id
  name String

  @@allow(\"read\", auth() != null)
  @@allow(\"create\", auth() != null)
  @@allow(\"update\", auth() != null)
  @@allow(\"delete\", auth() != null)
  @@internal(\"all\")
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names = file_names(&package);

    assert!(
        names.iter().all(|name| !name.contains("Gadget")),
        "an @@internal(\"all\") model must produce zero stub mappings under transport rpc, \
         got: {names:?}"
    );
}

/// `@version` models fan `update`/`delete` out into five `If-Match`-gated
/// stubs each (`model_state::version_gate::gated_mappings`), keyed
/// `"update"`, `"update-if-match-required"`, etc. Suppressing `update`
/// must drop all five, not just the bare `"update"` one — the canonical
/// verb (text before the first `-`) is what gets checked against
/// `model_internal_actions`, not the raw key.
#[test]
fn suppressing_update_on_a_versioned_model_drops_all_five_if_match_variants() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
  version Int @version

  @@allow(\"read\", auth() != null)
  @@allow(\"create\", auth() != null)
  @@allow(\"update\", auth() != null)
  @@allow(\"delete\", auth() != null)
  @@internal(\"update\")
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names = file_names(&package);

    for suffix in [
        "update",
        "update-if-match-required",
        "update-if-match-wildcard",
        "update-if-match-malformed",
        "update-if-match-stale",
    ] {
        let unexpected = format!("mappings/model.Widget.{suffix}.json");
        assert!(
            !names.contains(&unexpected.as_str()),
            "suppressed `update` must drop every If-Match variant, but found `{unexpected}` \
             in: {names:?}"
        );
    }
    // `delete`'s own If-Match variants are unaffected — only `update` was
    // suppressed.
    assert!(
        names.contains(&"mappings/model.Widget.delete.json"),
        "unsuppressed `delete` must still get a stub mapping, got: {names:?}"
    );
}
