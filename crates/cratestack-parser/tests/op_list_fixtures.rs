//! cratestack#1123: over every `.cstack` in the repository, the op list
//! (`cratestack_core::op_keys`) is exactly what the one verb list
//! (`model_verbs`) and the procedure keys say, with no duplicates and no
//! `@@internal` verb in it. The generators that cannot share the list in
//! their own language are pinned against `model_verbs` by the parity tests
//! next to them (`cratestack-client-{typescript,dart}/src/tests_op_list.rs`).

use std::collections::BTreeSet;

use cratestack_core::{
    ModelVerb, TransportStyle, model_internal_actions, model_op_key, model_verbs, op_keys,
    procedure_op_key,
};

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some("target" | "node_modules" | ".git")
            ) {
                walk(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "cstack") {
            out.push(path);
        }
    }
}

#[test]
fn the_op_list_is_the_verb_list_on_every_fixture() {
    let mut paths = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &mut paths,
    );
    let (mut schemas, mut ops) = (0, 0);
    for path in paths {
        let Ok(schema) = cratestack_parser::parse_schema_file(&path) else {
            continue;
        };
        let rpc = schema.transport == TransportStyle::Rpc;
        let mut expected = BTreeSet::new();
        for model in &schema.models {
            let internal = model_internal_actions(model);
            for verb in model_verbs(model) {
                assert!(
                    verb == ModelVerb::Subscribe || !internal.contains(verb.as_str()),
                    "{}: {} lists an @@internal verb",
                    path.display(),
                    model.name
                );
                expected.extend(model_op_key(&model.name, verb, rpc));
            }
        }
        expected.extend(schema.procedures.iter().map(|p| procedure_op_key(p, rpc)));
        let keys = op_keys(&schema);
        assert_eq!(
            keys.len(),
            expected.len(),
            "{}: duplicate op key",
            path.display()
        );
        assert_eq!(
            keys.into_iter().collect::<BTreeSet<_>>(),
            expected,
            "{}",
            path.display()
        );
        schemas += 1;
        ops += expected.len();
    }
    assert!(
        schemas > 100 && ops > 500,
        "walk found {schemas} schemas, {ops} ops"
    );
}
