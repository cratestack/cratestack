use std::path::PathBuf;

use super::schema_files_const;
use crate::include::parse::IncludedFile;

fn file(name: &str) -> IncludedFile {
    IncludedFile {
        manifest_relative: name.to_owned(),
        absolute: PathBuf::from("/m").join(name),
    }
}

#[test]
fn one_file_emits_nothing() {
    assert!(schema_files_const(&[file("schema.cstack")]).is_empty());
    assert!(schema_files_const(&[]).is_empty());
}

#[test]
fn three_files_emit_three_include_strs_in_order() {
    let out = schema_files_const(&[
        file("schema.cstack"),
        file("a/p.cstack"),
        file("b/q.cstack"),
    ])
    .to_string();
    assert_eq!(out.matches("include_str ! (\"/m/").count(), 3);
    let root = out.find("/m/schema.cstack").unwrap();
    let a = out.find("/m/a/p.cstack").unwrap();
    let b = out.find("/m/b/q.cstack").unwrap();
    assert!(root < a && a < b);
    assert!(out.contains("SCHEMA_FILES"));
}
