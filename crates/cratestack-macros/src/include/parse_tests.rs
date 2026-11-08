//! `included_files`: the `include_str!` path of each file is the path that was
//! actually read, however the schema literal is spelled
//! (docs/design/multi-file-schemas.md section 10).

use std::path::{Path, PathBuf};

use super::included_files;

const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\nauth Caller {\n  id Int\n}\n\n";

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cratestack-macros-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real/models")).unwrap();
        std::fs::create_dir_all(dir.join("crate")).unwrap();
        std::fs::write(
            dir.join("real/app.cstack"),
            format!("{HEAD}part \"models/t.cstack\"\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("real/models/t.cstack"),
            "part of \"../app.cstack\"\n\nmodel T {\n  id Int @id\n}\n",
        )
        .unwrap();
        Tree(dir)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn load(path: &Path) -> cratestack_parser::LoadedSchema {
    cratestack_parser::load_schema(path, &cratestack_parser::LoadOptions::default()).unwrap()
}

#[test]
fn an_absolute_literal_includes_the_files_where_they_are() {
    let tree = Tree::new("absolute");
    let root = tree.0.join("real/app.cstack");
    let loaded = load(&root);
    let manifest = tree.0.join("crate");
    let files = included_files(&manifest.display().to_string(), &loaded);
    for file in &files {
        assert!(
            file.absolute.is_file(),
            "{} exists",
            file.absolute.display()
        );
    }
    assert_eq!(files[1].manifest_relative, "../real/models/t.cstack");
}

#[cfg(unix)]
#[test]
fn a_symlinked_root_includes_the_real_files() {
    let tree = Tree::new("symlink");
    std::os::unix::fs::symlink(
        tree.0.join("real/app.cstack"),
        tree.0.join("crate/schema.cstack"),
    )
    .unwrap();
    let loaded = load(&tree.0.join("crate/schema.cstack"));
    let manifest = tree.0.join("crate");
    let files = included_files(&manifest.display().to_string(), &loaded);
    for file in &files {
        assert!(
            file.absolute.is_file(),
            "{} exists",
            file.absolute.display()
        );
    }
    let labels: Vec<_> = files.iter().map(|f| f.manifest_relative.as_str()).collect();
    assert_eq!(labels, ["../real/app.cstack", "../real/models/t.cstack"]);
}

#[test]
fn a_plain_relative_literal_keeps_its_manifest_relative_labels() {
    let tree = Tree::new("plain");
    let loaded = load(&tree.0.join("real/app.cstack"));
    let manifest = tree.0.join("real");
    let files = included_files(&manifest.display().to_string(), &loaded);
    let labels: Vec<_> = files.iter().map(|f| f.manifest_relative.as_str()).collect();
    assert_eq!(labels, ["app.cstack", "models/t.cstack"]);
}
