//! Placeholders must satisfy the field's own validators (cratestack#1307).
//! Each stub value is run through the real `cratestack_core::validate_*`
//! helpers (and the `regex` crate) the generated server code calls.

use cratestack_core::{
    validate_email, validate_iso4217, validate_length, validate_length_bytes, validate_range_i64,
    validate_uri,
};
use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};
use serde_json::Value;

const NO_DB: &str = "datasource db {\n  provider = \"none\"\n}\n";

const FIELDS: &str = "
type Line {
  amountMinor BigInt @range(min: 1)
  count Int @range(min: -10, max: -5)
  mid Int @range(min: 10, max: 20)
  currency String @iso4217
  note Bytes @length(min: 1, max: 8)
  name String @length(min: 12)
  short String @length(max: 3)
  mail String @email
  site String @uri
  code String @regex(\"^[A-Z]{3}-[0-9]{2}$\")
  slug String @regex(\"^[a-z]+$\") @length(min: 9)
}
";

fn args(attr: &str) -> (Option<i64>, Option<i64>) {
    let inner = attr
        .split_once('(')
        .map_or("", |(_, r)| r.trim_end_matches(')'));
    let get = |key: &str| {
        inner.split(',').find_map(|p| {
            let (k, v) = p.split_once(':')?;
            (k.trim() == key).then(|| v.trim().parse().ok())?
        })
    };
    (get("min"), get("max"))
}

fn check_line(schema: &cratestack_core::Schema, line: &Value) {
    let decl = schema.types.iter().find(|t| t.name == "Line").unwrap();
    for field in &decl.fields {
        let value = &line[&field.name];
        for attr in &field.attributes {
            let raw = attr.raw.as_str();
            let (min, max) = args(raw);
            let name = field.name.as_str();
            let ok = if raw.starts_with("@range") {
                let n = value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or_else(|| panic!("{name}: not an integer: {value}"));
                validate_range_i64(name, n, min, max).is_ok()
            } else if raw.starts_with("@length") && field.ty.name == "Bytes" {
                let bytes: Vec<u8> = serde_json::from_value(value.clone()).unwrap();
                let (min, max) = (min.map(|n| n as usize), max.map(|n| n as usize));
                validate_length_bytes(name, &bytes, min, max).is_ok()
            } else if raw.starts_with("@length") {
                let (min, max) = (min.map(|n| n as usize), max.map(|n| n as usize));
                validate_length(name, value.as_str().unwrap(), min, max).is_ok()
            } else if raw.starts_with("@email") {
                validate_email(name, value.as_str().unwrap()).is_ok()
            } else if raw.starts_with("@uri") {
                validate_uri(name, value.as_str().unwrap()).is_ok()
            } else if raw.starts_with("@iso4217") {
                validate_iso4217(name, value.as_str().unwrap()).is_ok()
            } else if let Some(p) = raw.strip_prefix("@regex(\"") {
                let re = regex::Regex::new(p.strip_suffix("\")").unwrap()).unwrap();
                re.is_match(value.as_str().unwrap())
            } else {
                true
            };
            assert!(ok, "{name} = {value} violates {raw}");
        }
    }
}

#[test]
fn procedure_placeholders_satisfy_validators_on_rest_and_rpc() {
    for transport in ["", "transport rpc\n"] {
        let src = format!(
            "{NO_DB}{transport}{FIELDS}
type Out {{
  line Line
  lines Line[]
}}
procedure get(input: Out): Out
"
        );
        let schema = cratestack_parser::parse_schema(&src).expect("parses");
        let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
        let file = package
            .files
            .iter()
            .find(|f| f.file_name.ends_with("get.json"))
            .unwrap();
        let mapping: Value = serde_json::from_str(&file.contents).unwrap();
        let body = &mapping["response"]["jsonBody"];
        check_line(&schema, &body["line"]);
        check_line(&schema, &body["lines"][0]);
    }
}

#[test]
fn model_stubs_use_validator_satisfying_defaults() {
    let src = format!(
        "{NO_DB}
model Account {{
  id Int @id
  currency String @iso4217
  amount BigInt @range(min: 5)
}}
"
    );
    let src = src.replace(
        "provider = \"none\"",
        "provider = \"postgresql\"\n  url = env(\"U\")",
    );
    let schema = cratestack_parser::parse_schema(&src).expect("parses");
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let all: String = package.files.iter().map(|f| f.contents.as_str()).collect();
    assert!(
        all.contains("EUR") && !all.contains("\\\"string\\\""),
        "{all}"
    );
    assert!(
        !all.contains("\\\"0\\\""),
        "BigInt default must honour @range: {all}"
    );
}

const KEY: &str = "cratestackUnsatisfiedValidators";

fn mappings(src: &str) -> Vec<(String, Value)> {
    let schema = cratestack_parser::parse_schema(src).expect("parses");
    let package = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect("generation still succeeds");
    package
        .files
        .iter()
        .map(|f| {
            (
                f.file_name.clone(),
                serde_json::from_str(&f.contents).unwrap(),
            )
        })
        .collect()
}

#[test]
fn unsatisfiable_procedure_validators_are_noted_in_metadata() {
    for transport in ["", "transport rpc\n"] {
        let src = format!(
            "{NO_DB}{transport}
type T {{
  m String @regex(\"^a$\") @length(min: 3)
  ok String @iso4217
}}
type Out {{
  t T
}}
procedure p(input: T): Out
"
        );
        let all = mappings(&src);
        let mapping = &all.iter().find(|(n, _)| n.ends_with("p.json")).unwrap().1;
        // The best candidate (ties go to the first) is still emitted.
        assert_eq!(mapping["response"]["jsonBody"]["t"]["m"], "a");
        assert_eq!(mapping["response"]["jsonBody"]["t"]["ok"], "EUR");
        assert_eq!(
            mapping["metadata"][KEY],
            serde_json::json!([{ "path": "t.m", "validators": ["@length(min: 3)"] }]),
        );
        assert_eq!(mapping["metadata"]["cratestack"]["generated"], true);
    }
}

#[test]
fn unsatisfiable_model_validators_are_noted_on_every_model_mapping() {
    let src = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"U\")\n}\n
model Account {
  id Int @id
  code String @email @length(max: 4)
}
";
    for transport in ["", "transport rpc\n"] {
        let all = mappings(&format!("{transport}{src}"));
        assert!(all.len() >= 5);
        for (name, mapping) in all {
            let note = &mapping["metadata"][KEY];
            assert_eq!(note[0]["path"], "code", "{name}");
            assert_eq!(
                note[0]["validators"],
                serde_json::json!(["@length(max: 4)"]),
                "{name}"
            );
        }
    }
}

#[test]
fn satisfiable_schemas_carry_no_note() {
    let all = mappings(&format!(
        "{NO_DB}{FIELDS}\ntype Out {{\n  line Line\n}}\nprocedure g(input: Out): Out\n"
    ));
    assert!(all.iter().all(|(_, m)| m["metadata"].get(KEY).is_none()));
}
