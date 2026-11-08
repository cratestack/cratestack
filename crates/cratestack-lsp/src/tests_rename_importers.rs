//! A shared file with no directive of its own, opened alone, is still one
//! file of every schema that imports it: renaming in it must be refused when
//! any `.cstack` file in the workspace points at it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp_server::jsonrpc;

use crate::importers::check_not_imported;

const MESSAGE: &str = "rename is not available yet in a schema split across files";

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = base.join(format!("cratestack-lsp-imp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("shared")).unwrap();
        std::fs::write(dir.join("shared/common.cstack"), "enum Money {\n  X\n}\n").unwrap();
        Tree(dir)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn roots(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }

    fn common(&self) -> PathBuf {
        self.0.join("shared/common.cstack")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn refused(roots: &[PathBuf], target: &Path, open: &HashMap<PathBuf, String>) -> bool {
    match check_not_imported(roots, target, open) {
        Ok(()) => false,
        Err(error) => {
            assert_eq!(jsonrpc::Error::from(error).message, MESSAGE);
            true
        }
    }
}

#[test]
fn a_file_imported_by_a_root_on_disk_is_refused() {
    let tree = Tree::new("disk");
    tree.write("app/schema.cstack", "import \"../shared/common.cstack\"\n");
    assert!(refused(&tree.roots(), &tree.common(), &HashMap::new()));
}

#[test]
fn an_unsaved_import_in_an_open_buffer_is_refused() {
    let tree = Tree::new("buffer");
    let root = tree.write("app/schema.cstack", "model M {\n  id Int @id\n}\n");
    assert!(!refused(&tree.roots(), &tree.common(), &HashMap::new()));
    let open = HashMap::from([(
        root,
        "import \"../shared/common.cstack\"\nmodel M {\n  id Int @id\n}\n".to_owned(),
    )]);
    assert!(refused(&tree.roots(), &tree.common(), &open));
}

#[test]
fn a_saved_import_removed_in_the_open_buffer_is_not_a_reason() {
    let tree = Tree::new("removed");
    let root = tree.write("app/schema.cstack", "import \"../shared/common.cstack\"\n");
    let open = HashMap::from([(root, "model M {\n  id Int @id\n}\n".to_owned())]);
    assert!(!refused(&tree.roots(), &tree.common(), &open));
}

#[test]
fn a_file_listed_by_a_part_line_is_refused() {
    let tree = Tree::new("part");
    tree.write("schema.cstack", "part \"shared/common.cstack\"\n");
    assert!(refused(&tree.roots(), &tree.common(), &HashMap::new()));
}

#[test]
fn unrelated_schemas_and_same_named_files_elsewhere_are_not_refused() {
    let tree = Tree::new("unrelated");
    tree.write("solo/schema.cstack", "model S {\n  id Int @id\n}\n");
    // Same file name, a different directory: resolves to `other/common.cstack`.
    tree.write("other/common.cstack", "enum Money {\n  Y\n}\n");
    tree.write("other/schema.cstack", "import \"common.cstack\"\n");
    assert!(!refused(&tree.roots(), &tree.common(), &HashMap::new()));
}

#[test]
fn without_workspace_roots_nothing_is_known_and_nothing_is_refused() {
    let tree = Tree::new("noroots");
    tree.write("app/schema.cstack", "import \"../shared/common.cstack\"\n");
    assert!(!refused(&[], &tree.common(), &HashMap::new()));
}

#[test]
fn hidden_and_build_directories_are_not_searched() {
    let tree = Tree::new("skipped");
    tree.write(
        "node_modules/x/schema.cstack",
        "import \"../../shared/common.cstack\"\n",
    );
    tree.write(".git/schema.cstack", "import \"../shared/common.cstack\"\n");
    assert!(!refused(&tree.roots(), &tree.common(), &HashMap::new()));
}

#[cfg(unix)]
#[test]
fn a_symlinked_spelling_of_the_shared_file_is_refused() {
    let tree = Tree::new("symlink");
    std::os::unix::fs::symlink(tree.0.join("shared"), tree.0.join("alias")).unwrap();
    tree.write("app/schema.cstack", "import \"../alias/common.cstack\"\n");
    assert!(refused(&tree.roots(), &tree.common(), &HashMap::new()));
    // And the other way: the editor opened the alias spelling.
    assert!(refused(
        &tree.roots(),
        &tree.0.join("alias/common.cstack"),
        &HashMap::new()
    ));
}
