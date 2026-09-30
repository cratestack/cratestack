//! cratestack#1123: the Dart client's per-model verb gates must equal
//! `cratestack_core::model_verbs`, over every `.cstack` in the repository.

use cratestack_core::{ModelVerb, model_verbs};

use crate::builders_model::build_model_api;

fn fixtures() -> Vec<cratestack_core::Schema> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if !matches!(name.to_str(), Some("target" | "node_modules" | ".git")) {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "cstack") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .iter()
        .filter_map(|p| cratestack_parser::parse_schema_file(p).ok())
        .collect()
}

#[test]
fn verb_gates_equal_the_op_list_on_every_fixture() {
    let mut models = 0;
    for schema in fixtures() {
        for model in &schema.models {
            let verbs = model_verbs(model);
            let api = build_model_api(model);
            let has = |verb| verbs.contains(&verb);
            assert_eq!(api.allows_list, has(ModelVerb::List), "{}", model.name);
            assert_eq!(api.allows_get, has(ModelVerb::Get), "{}", model.name);
            assert_eq!(api.allows_create, has(ModelVerb::Create), "{}", model.name);
            assert_eq!(api.allows_update, has(ModelVerb::Update), "{}", model.name);
            assert_eq!(api.allows_delete, has(ModelVerb::Delete), "{}", model.name);
            models += 1;
        }
    }
    assert!(models > 100, "the fixture walk found only {models} models");
}
