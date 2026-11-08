//! Symlinks and the one resolution rule (docs/design/multi-file-schemas.md
//! section 5, Q3): every directive resolves against the directory of the
//! canonical path of the file that holds it, whichever spelling reached it.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use cratestack_parser::{LoadOptions, LoadedSchema, SchemaError, load_schema};

const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\nauth Caller {\n  id Int\n}\n\n";

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cratestack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tree(dir)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn link(&self, to: &str, at: &str) {
        let path = self.0.join(at);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(self.0.join(to), path).unwrap();
    }

    fn load(&self, rel: &str) -> Result<LoadedSchema, SchemaError> {
        load_schema(self.0.join(rel), &LoadOptions::default())
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn nested_import_tree(name: &str, imports: &str) -> Tree {
    let tree = Tree::new(name);
    tree.write("schema.cstack", &format!("{HEAD}{imports}"));
    tree.write(
        "shared/c.cstack",
        "import \"d.cstack\"\n\ntype C {\n  d D\n}\n",
    );
    tree.write("shared/d.cstack", "type D {\n  x Int\n}\n");
    tree.link("shared/c.cstack", "link.cstack");
    tree
}

#[test]
fn nested_imports_of_a_symlink_resolve_from_the_real_directory_either_order() {
    let real_first = nested_import_tree(
        "sym-order-a",
        "import \"shared/c.cstack\"\nimport \"link.cstack\"\n",
    );
    let link_first = nested_import_tree(
        "sym-order-b",
        "import \"link.cstack\"\nimport \"shared/c.cstack\"\n",
    );
    for tree in [&real_first, &link_first] {
        let loaded = tree
            .load("schema.cstack")
            .expect("d.cstack is next to c.cstack");
        let names: Vec<_> = loaded.schema.types.iter().map(|t| t.name.clone()).collect();
        assert_eq!(loaded.files.len(), 3, "{names:?}");
        assert!(names.contains(&"D".to_owned()), "{names:?}");
    }
}

#[test]
fn a_link_alone_loads_the_same_as_the_real_file() {
    let tree = nested_import_tree("sym-alone", "import \"link.cstack\"\n");
    let loaded = tree.load("schema.cstack").expect("resolved from shared/");
    assert_eq!(loaded.files.len(), 3);
    for file in &loaded.files {
        assert!(
            Path::new(&file.display).exists(),
            "{} is real",
            file.display
        );
    }
}

#[test]
fn dotdot_through_a_symlinked_directory_means_what_the_os_means() {
    let tree = Tree::new("sym-dotdot");
    tree.write(
        "schema.cstack",
        &format!("{HEAD}import \"via/../d.cstack\"\n"),
    );
    tree.write("real/deep/keep.txt", "");
    tree.write("real/d.cstack", "type D {\n  x Int\n}\n");
    tree.write("d.cstack", "type Wrong {\n  x Int\n}\n");
    tree.link("real/deep", "via");
    let loaded = tree.load("schema.cstack").expect("via/.. is real/");
    let names: Vec<_> = loaded.schema.types.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, ["D"]);
}

#[test]
fn a_symlinked_root_displays_its_members_where_they_really_are() {
    let tree = Tree::new("sym-root");
    tree.write(
        "real/app.cstack",
        &format!("{HEAD}part \"models/t.cstack\"\n"),
    );
    tree.write(
        "real/models/t.cstack",
        "part of \"../app.cstack\"\n\nmodel T {\n  id Int @id\n}\n",
    );
    tree.link("real/app.cstack", "crate/schema.cstack");
    let loaded = tree
        .load("crate/schema.cstack")
        .expect("loads through the link");
    assert!(loaded.files[0].display.ends_with("crate/schema.cstack"));
    let part = &loaded.files[1];
    assert!(Path::new(&part.display).exists(), "{} exists", part.display);
    assert_eq!(
        part.path,
        std::fs::canonicalize(tree.0.join("real/models/t.cstack")).unwrap()
    );
    assert_eq!(part.relative, "models/t.cstack");
}
