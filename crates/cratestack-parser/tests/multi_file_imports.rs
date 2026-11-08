//! `import`: transitive, flat, deduplicated by canonical path
//! (docs/design/multi-file-schemas.md sections 5 and 7).

mod multi_file_support;

use cratestack_parser::{FileRole, LoadOptions, load_schema};
use multi_file_support::{HEAD, ROOT, load};

const MONEY: &str = "/fixture/shared/money.cstack";
const CURRENCY: &str = "/fixture/shared/money/currency.cstack";
const IDS: &str = "/fixture/shared/ids.cstack";
const ID_TYPE: &str = "type Id {\n  value Int\n}\n";

fn roles(files: &[(&str, &str)]) -> Vec<(String, FileRole)> {
    load(files)
        .unwrap()
        .files
        .iter()
        .map(|f| (f.relative.clone(), f.role))
        .collect()
}

#[test]
fn an_import_brings_types_enums_and_mixins_in() {
    let root = format!(
        "{HEAD}import \"shared/money.cstack\"\n\nmodel Invoice {{\n  @use(Stamps)\n  id Int @id\n  \
         currency Currency\n}}\n"
    );
    let money = "enum Currency {\n  EUR\n  XAF\n}\n\ntype Money {\n  currency Currency\n}\n\n\
                 mixin Stamps {\n  createdAt DateTime\n}\n";
    let schema = load(&[(ROOT, root.as_str()), (MONEY, money)])
        .unwrap()
        .schema;
    assert_eq!(schema.enums[0].name, "Currency");
    assert_eq!(schema.types[0].name, "Money");
    let fields: Vec<_> = schema.models[0].fields.iter().map(|f| &*f.name).collect();
    assert!(fields.contains(&"createdAt"), "{fields:?}");
}

#[test]
fn imports_are_transitive() {
    let root = format!("{HEAD}import \"a.cstack\"\n\ntype Quote {{\n  price Money\n}}\n");
    let a = "import \"base.cstack\"\n\ntype Money {\n  currency Currency\n}\n";
    let base = "enum Currency {\n  EUR\n}\n";
    let files = [
        (ROOT, root.as_str()),
        ("/fixture/a.cstack", a),
        ("/fixture/base.cstack", base),
    ];
    let schema = load(&files).unwrap().schema;
    let names: Vec<_> = schema.types.iter().map(|t| &*t.name).collect();
    assert_eq!(names, ["Quote", "Money"]);
    assert_eq!(schema.enums[0].name, "Currency");
}

#[test]
fn an_imported_root_may_have_parts() {
    let root = format!("{HEAD}import \"shared/money.cstack\"\n");
    let money = "part \"money/currency.cstack\"\n\ntype Money {\n  currency Currency\n}\n";
    let currency = "part of \"../money.cstack\"\n\nenum Currency {\n  EUR\n}\n";
    let files = [(ROOT, root.as_str()), (MONEY, money), (CURRENCY, currency)];
    let loaded = load(&files).unwrap();
    assert_eq!(
        roles(&files),
        [
            ("schema.cstack".to_owned(), FileRole::Root),
            ("shared/money.cstack".to_owned(), FileRole::Import),
            (
                "shared/money/currency.cstack".to_owned(),
                FileRole::ImportPart
            ),
        ]
    );
    assert_eq!(loaded.schema.enums[0].name, "Currency");
}

#[test]
fn imports_load_after_parts_in_pre_order() {
    let root = format!(
        "{HEAD}import \"shared/money.cstack\"\npart \"p.cstack\"\nimport \"shared/ids.cstack\"\n"
    );
    let p =
        "part of \"schema.cstack\"\nimport \"shared/ids.cstack\"\n\ntype FromPart {\n  x Int\n}\n";
    let money = "import \"ids.cstack\"\n\ntype Money {\n  x Int\n}\n";
    let files = [
        (ROOT, root.as_str()),
        ("/fixture/p.cstack", p),
        (MONEY, money),
        (IDS, ID_TYPE),
    ];
    // Root, its part, then the root's imports in order (money, which brings
    // ids with it), then the part's import (ids: already loaded, skipped).
    assert_eq!(
        roles(&files),
        [
            ("schema.cstack".to_owned(), FileRole::Root),
            ("p.cstack".to_owned(), FileRole::Part),
            ("shared/money.cstack".to_owned(), FileRole::Import),
            ("shared/ids.cstack".to_owned(), FileRole::Import),
        ]
    );
    let names: Vec<_> = load(&files)
        .unwrap()
        .schema
        .types
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["FromPart", "Money", "Id"]);
}

#[test]
fn a_diamond_loads_the_shared_file_once() {
    let root = format!("{HEAD}import \"x/a.cstack\"\nimport \"x/b.cstack\"\n");
    let a = "import \"../shared/c.cstack\"\n";
    let b = "import \"../shared/./c.cstack\"\n";
    let files = [
        (ROOT, root.as_str()),
        ("/fixture/x/a.cstack", a),
        ("/fixture/x/b.cstack", b),
        ("/fixture/shared/c.cstack", ID_TYPE),
    ];
    let loaded = load(&files).expect("one copy of c, so no collision");
    assert_eq!(loaded.files.len(), 4);
    assert_eq!(loaded.schema.types.len(), 1);
}

#[cfg(unix)]
#[test]
fn a_symlinked_spelling_dedupes_too() {
    let dir =
        std::env::temp_dir().join(format!("cratestack-import-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("shared")).unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("shared/c.cstack"), ID_TYPE).unwrap();
    std::os::unix::fs::symlink(dir.join("shared/c.cstack"), dir.join("link.cstack")).unwrap();
    let root = format!(
        "{HEAD}import \"shared/c.cstack\"\nimport \"link.cstack\"\nimport \"sub/../shared/c.cstack\"\n"
    );
    std::fs::write(dir.join("schema.cstack"), root).unwrap();
    let loaded = load_schema(dir.join("schema.cstack"), &LoadOptions::default());
    let _ = std::fs::remove_dir_all(&dir);
    let loaded = loaded.expect("three spellings, one file");
    assert_eq!(loaded.files.len(), 2);
    assert_eq!(loaded.schema.types.len(), 1);
}
